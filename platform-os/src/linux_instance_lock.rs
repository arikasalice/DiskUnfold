//! Per-data-directory exclusion for the Linux GUI writer.

use std::{error::Error, fmt, fs::File, io, os::fd::AsFd, path::Path};

use rustix::fs::{FileType, FlockOperation, Mode, OFlags, flock, fstat, openat};

const LOCK_FILE: &str = ".diskunfold.instance.lock";

/// Failure to acquire or validate the per-directory Linux GUI guard.
#[derive(Debug)]
pub enum LinuxInstanceLockError {
    /// Another cooperating GUI process holds the lock.
    Busy,
    /// The lock sentinel is not a single-link regular file.
    InvalidSentinel,
    /// The database path is not a single-link regular file.
    InvalidDatabase,
    /// A requested child path is not a real directory.
    InvalidDirectory,
    /// A native file operation failed.
    Io(io::Error),
}

impl fmt::Display for LinuxInstanceLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => formatter.write_str("another GUI instance holds the directory lock"),
            Self::InvalidSentinel => formatter.write_str("invalid GUI instance lock file"),
            Self::InvalidDatabase => formatter.write_str("invalid database file"),
            Self::InvalidDirectory => formatter.write_str("invalid child directory"),
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for LinuxInstanceLockError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// Holds an advisory lock and a directory handle for one application's data directory.
///
/// All GUI writers must use this same sentinel before opening or migrating the database.
pub struct LinuxInstanceLock {
    directory: File,
    _sentinel: File,
}

impl LinuxInstanceLock {
    /// Acquires the non-blocking exclusive lock for an existing data directory.
    pub fn acquire(path: &Path) -> Result<Self, LinuxInstanceLockError> {
        let directory = File::open(path).map_err(LinuxInstanceLockError::Io)?;
        let sentinel = openat(
            &directory,
            LOCK_FILE,
            OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|error| match error {
            rustix::io::Errno::LOOP => LinuxInstanceLockError::InvalidSentinel,
            error => LinuxInstanceLockError::Io(error.into()),
        })?;
        let sentinel = File::from(sentinel);
        if !single_link_regular_file(&sentinel)? {
            return Err(LinuxInstanceLockError::InvalidSentinel);
        }
        flock(&sentinel, FlockOperation::NonBlockingLockExclusive).map_err(|error| {
            if error == rustix::io::Errno::WOULDBLOCK || error == rustix::io::Errno::AGAIN {
                LinuxInstanceLockError::Busy
            } else {
                LinuxInstanceLockError::Io(error.into())
            }
        })?;
        Ok(Self {
            directory,
            _sentinel: sentinel,
        })
    }

    /// Pins an existing regular database file or reports that the path is absent.
    pub fn pin_database(&self) -> Result<Option<File>, LinuxInstanceLockError> {
        let file = match openat(
            &self.directory,
            crate::application_data::DATABASE_FILE,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(file) => File::from(file),
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(rustix::io::Errno::LOOP) => return Err(LinuxInstanceLockError::InvalidDatabase),
            Err(error) => return Err(LinuxInstanceLockError::Io(error.into())),
        };
        if !single_link_regular_file(&file)? {
            return Err(LinuxInstanceLockError::InvalidDatabase);
        }
        Ok(Some(file))
    }

    /// Opens a direct child directory without following a symbolic link.
    ///
    /// The child name must be one path component. The returned descriptor pins
    /// the directory object while callers perform managed startup operations.
    pub fn open_child_directory(&self, name: &str) -> Result<File, LinuxInstanceLockError> {
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(LinuxInstanceLockError::InvalidDirectory);
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
        .map_err(|error| match error {
            rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR => {
                LinuxInstanceLockError::InvalidDirectory
            }
            error => LinuxInstanceLockError::Io(error.into()),
        })?;
        let directory = File::from(directory);
        let stat =
            fstat(directory.as_fd()).map_err(|error| LinuxInstanceLockError::Io(error.into()))?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
            return Err(LinuxInstanceLockError::InvalidDirectory);
        }
        Ok(directory)
    }
}

fn single_link_regular_file(file: &File) -> Result<bool, LinuxInstanceLockError> {
    let stat = fstat(file.as_fd()).map_err(|error| LinuxInstanceLockError::Io(error.into()))?;
    Ok(FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile && stat.st_nlink == 1)
}

#[cfg(test)]
mod tests {
    use super::{LinuxInstanceLock, LinuxInstanceLockError};
    use std::{fs, path::PathBuf};

    use tempfile::TempDir;

    fn data_dir() -> Result<(TempDir, PathBuf), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let data = temp.path().join("DiskUnfold");
        fs::create_dir(&data)?;
        Ok((temp, data))
    }

    #[test]
    fn lock_excludes_a_second_gui_and_releases_on_drop() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp, data) = data_dir()?;
        let first = LinuxInstanceLock::acquire(&data)?;
        assert!(matches!(
            LinuxInstanceLock::acquire(&data),
            Err(LinuxInstanceLockError::Busy)
        ));
        drop(first);
        assert!(LinuxInstanceLock::acquire(&data).is_ok());
        Ok(())
    }

    #[test]
    fn lock_rejects_a_symlink_sentinel() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp, data) = data_dir()?;
        let target = data.join("other");
        fs::write(&target, b"not the lock")?;
        std::os::unix::fs::symlink(&target, data.join(".diskunfold.instance.lock"))?;
        assert!(matches!(
            LinuxInstanceLock::acquire(&data),
            Err(LinuxInstanceLockError::InvalidSentinel)
        ));
        Ok(())
    }

    #[test]
    fn database_pin_rejects_hard_links() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp, data) = data_dir()?;
        let database = data.join(crate::application_data::DATABASE_FILE);
        fs::write(&database, b"database")?;
        fs::hard_link(&database, data.join("database-alias"))?;
        let lock = LinuxInstanceLock::acquire(&data)?;
        assert!(matches!(
            lock.pin_database(),
            Err(LinuxInstanceLockError::InvalidDatabase)
        ));
        Ok(())
    }
}
