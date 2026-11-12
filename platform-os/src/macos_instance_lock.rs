//! Per-data-directory exclusion for the macOS GUI writer.

use std::{
    error::Error,
    fmt,
    fs::{self, File},
    io,
    os::unix::fs::MetadataExt,
    path::Path,
};

use rustix::{
    fs::{CWD, FlockOperation, Mode, OFlags, flock, openat},
    io::Errno,
};

const LOCK_FILE: &str = ".diskunfold.instance.lock";

/// Failure to acquire or validate the per-directory macOS GUI guard.
#[derive(Debug)]
pub enum MacOsInstanceLockError {
    /// Another cooperating GUI process holds the lock.
    Busy,
    /// The data directory is not an existing, real directory.
    InvalidDirectory,
    /// The lock sentinel is not a single-link regular file.
    InvalidSentinel,
    /// The database path is not a single-link regular file.
    InvalidDatabase,
    /// A requested child path is not a real directory.
    InvalidChildDirectory,
    /// A checked path no longer names the object held by this guard.
    PathChanged,
    /// A native file operation failed.
    Io(io::Error),
}

impl fmt::Display for MacOsInstanceLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => formatter.write_str("another GUI instance holds the directory lock"),
            Self::InvalidDirectory => formatter.write_str("invalid application data directory"),
            Self::InvalidSentinel => formatter.write_str("invalid GUI instance lock file"),
            Self::InvalidDatabase => formatter.write_str("invalid database file"),
            Self::InvalidChildDirectory => formatter.write_str("invalid child directory"),
            Self::PathChanged => {
                formatter.write_str("a managed database path changed during startup")
            }
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for MacOsInstanceLockError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// Holds an advisory lock and a descriptor for one application's data directory.
///
/// File operations exposed by this guard resolve direct children relative to the
/// pinned directory descriptor. The lock coordinates cooperating GUI processes
/// while the sentinel remains linked at its expected name. It does not protect
/// path-based SQLite opens from an unrelated directory or database replacement,
/// and unrelated filesystem mutation is outside its exclusion guarantee.
pub struct MacOsInstanceLock {
    directory: File,
    _sentinel: File,
}

impl MacOsInstanceLock {
    /// Acquires the non-blocking exclusive lock for an existing absolute data directory.
    ///
    /// The final directory component is opened without following a symbolic link.
    pub fn acquire(path: &Path) -> Result<Self, MacOsInstanceLockError> {
        if !path.is_absolute() {
            return Err(MacOsInstanceLockError::InvalidDirectory);
        }
        let directory = openat(
            CWD,
            path,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::CLOEXEC
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|error| {
            if error == Errno::LOOP || error == Errno::NOTDIR {
                MacOsInstanceLockError::InvalidDirectory
            } else {
                MacOsInstanceLockError::Io(error.into())
            }
        })?;
        let directory = File::from(directory);
        if !directory
            .metadata()
            .map_err(MacOsInstanceLockError::Io)?
            .is_dir()
        {
            return Err(MacOsInstanceLockError::InvalidDirectory);
        }

        let sentinel = openat(
            &directory,
            LOCK_FILE,
            OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|error| {
            if error == Errno::LOOP || error == Errno::ISDIR {
                MacOsInstanceLockError::InvalidSentinel
            } else {
                MacOsInstanceLockError::Io(error.into())
            }
        })?;
        let sentinel = File::from(sentinel);
        if !single_link_regular_file(&sentinel)? {
            return Err(MacOsInstanceLockError::InvalidSentinel);
        }
        flock(&sentinel, FlockOperation::NonBlockingLockExclusive).map_err(|error| {
            if error == Errno::WOULDBLOCK || error == Errno::AGAIN {
                MacOsInstanceLockError::Busy
            } else {
                MacOsInstanceLockError::Io(error.into())
            }
        })?;

        Ok(Self {
            directory,
            _sentinel: sentinel,
        })
    }

    /// Opens and pins an existing regular database file or reports that it is absent.
    ///
    /// The returned handle remains bound to the opened file object. It does not
    /// bind a later SQLite open performed by path, or exclude a same-user path
    /// replacement between this check and a caller's path-based open.
    pub fn pin_database(&self) -> Result<Option<File>, MacOsInstanceLockError> {
        let file = match openat(
            &self.directory,
            crate::application_data::DATABASE_FILE,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(file) => File::from(file),
            Err(Errno::NOENT) => return Ok(None),
            Err(Errno::LOOP) | Err(Errno::ISDIR) => {
                return Err(MacOsInstanceLockError::InvalidDatabase);
            }
            Err(error) => return Err(MacOsInstanceLockError::Io(error.into())),
        };
        if !single_link_regular_file(&file)? {
            return Err(MacOsInstanceLockError::InvalidDatabase);
        }
        Ok(Some(file))
    }

    /// Checks that the configured data-directory path still names the pinned directory.
    ///
    /// This detects a persistent replacement at a startup checkpoint. It does not prevent a
    /// same-user process from replacing the path immediately before a SQLite path open.
    pub fn verify_data_directory_path(&self, path: &Path) -> Result<(), MacOsInstanceLockError> {
        if !path.is_absolute() {
            return Err(MacOsInstanceLockError::InvalidDirectory);
        }
        let path_metadata = fs::symlink_metadata(path).map_err(MacOsInstanceLockError::Io)?;
        let pinned_metadata = self
            .directory
            .metadata()
            .map_err(MacOsInstanceLockError::Io)?;
        if !path_metadata.is_dir()
            || path_metadata.file_type().is_symlink()
            || !same_file(&path_metadata, &pinned_metadata)
        {
            return Err(MacOsInstanceLockError::PathChanged);
        }
        Ok(())
    }

    /// Checks that the sentinel name still identifies the file whose advisory lock is held.
    ///
    /// This catches an unlinked or persistently replaced sentinel, but the advisory lock alone
    /// cannot exclude an unrelated process that mutates files in this same-user directory.
    pub fn verify_sentinel_path(&self) -> Result<(), MacOsInstanceLockError> {
        let current = openat(
            &self.directory,
            LOCK_FILE,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|error| {
            if error == Errno::NOENT || error == Errno::LOOP || error == Errno::ISDIR {
                MacOsInstanceLockError::PathChanged
            } else {
                MacOsInstanceLockError::Io(error.into())
            }
        })?;
        let current = File::from(current);
        let current_metadata = current.metadata().map_err(MacOsInstanceLockError::Io)?;
        let held_metadata = self
            ._sentinel
            .metadata()
            .map_err(MacOsInstanceLockError::Io)?;
        if !current_metadata.is_file()
            || current_metadata.nlink() != 1
            || !same_file(&current_metadata, &held_metadata)
        {
            return Err(MacOsInstanceLockError::PathChanged);
        }
        Ok(())
    }

    /// Checks that the database name still identifies the pinned regular file.
    ///
    /// This detects a persistent replacement at a startup checkpoint. It does not bind a later
    /// SQLite path open to the pinned descriptor or close the check/open race.
    pub fn verify_database_path(&self, pinned: &File) -> Result<(), MacOsInstanceLockError> {
        let current = openat(
            &self.directory,
            crate::application_data::DATABASE_FILE,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|error| {
            if error == Errno::NOENT || error == Errno::LOOP || error == Errno::ISDIR {
                MacOsInstanceLockError::PathChanged
            } else {
                MacOsInstanceLockError::Io(error.into())
            }
        })?;
        let current = File::from(current);
        if !single_link_regular_file(&current)? {
            return Err(MacOsInstanceLockError::InvalidDatabase);
        }
        let current_metadata = current.metadata().map_err(MacOsInstanceLockError::Io)?;
        let pinned_metadata = pinned.metadata().map_err(MacOsInstanceLockError::Io)?;
        if !same_file(&current_metadata, &pinned_metadata) {
            return Err(MacOsInstanceLockError::PathChanged);
        }
        Ok(())
    }

    /// Opens a direct child directory without following a symbolic link.
    ///
    /// The child name must be one path component. The returned descriptor pins
    /// the directory object while callers perform managed startup operations.
    pub fn open_child_directory(&self, name: &str) -> Result<File, MacOsInstanceLockError> {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || name.as_bytes().contains(&0)
        {
            return Err(MacOsInstanceLockError::InvalidChildDirectory);
        }
        let directory = openat(
            &self.directory,
            name,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::CLOEXEC
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|error| {
            if error == Errno::LOOP || error == Errno::NOTDIR {
                MacOsInstanceLockError::InvalidChildDirectory
            } else {
                MacOsInstanceLockError::Io(error.into())
            }
        })?;
        let directory = File::from(directory);
        if !directory
            .metadata()
            .map_err(MacOsInstanceLockError::Io)?
            .is_dir()
        {
            return Err(MacOsInstanceLockError::InvalidChildDirectory);
        }
        Ok(directory)
    }
}

fn single_link_regular_file(file: &File) -> Result<bool, MacOsInstanceLockError> {
    let metadata = file.metadata().map_err(MacOsInstanceLockError::Io)?;
    Ok(metadata.is_file() && metadata.nlink() == 1)
}

fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(test)]
mod tests;
