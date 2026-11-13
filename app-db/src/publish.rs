//! Publication of a newly created database under its final name (FR3.HD1.app-db.DD1).
//!
//! The creator builds a complete database under a temporary name in the same folder and
//! then publishes it. The primary route is a no-overwrite hard link. A file system that
//! cannot hard link (FAT, exFAT and some network shares) uses the marker route:
//!
//! 1. write `<name>.creating` (exclusive),
//! 2. copy the complete temporary file to `<name>` (exclusive, never overwriting),
//! 3. write the completion marker `<name>.created` last, holding the database length,
//! 4. remove `<name>.creating`.
//!
//! A database that has a `.creating` file but no valid marker was interrupted. It is never
//! opened: [`recover_incomplete_creation`] removes it so that the creator builds it again.
//! A database without a `.creating` file was published by the hard link route or finished
//! the marker route, so databases created before this fallback existed stay valid.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::{APPLICATION_ID, AppDb, BUSY_TIMEOUT, OpenError};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Observable points of the marker route, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// `<name>.creating` exists; the database file does not yet.
    SentinelWritten,
    /// The database file exists but is still empty.
    DatabaseCreated,
    /// The complete copy is on disk; no marker yet.
    DatabaseCopied,
    /// The marker exists; `.creating` still exists.
    MarkerWritten,
    /// Publication is finished; the temporary file may still exist.
    SentinelRemoved,
}

/// Test hook result: an I/O failure the process survives, or a kill that leaves every file as is.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum Interrupt {
    Fail(io::Error),
    Crash,
}

/// What [`recover_incomplete_creation`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreationRecovery {
    /// No interrupted marker-route creation exists; nothing was changed.
    NotApplicable,
    /// The database was complete; only the leftover `.creating` file was removed.
    Completed,
    /// The half-built database and its files were removed; create the database again.
    Discarded,
}

pub(crate) struct PendingFile {
    pub(crate) path: PathBuf,
    pub(crate) cleaned: bool,
}
impl Drop for PendingFile {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn reserve_temporary(path: &Path) -> Result<PendingFile, OpenError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    loop {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".diskunfold-create-{}-{id}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => {
                return Ok(PendingFile {
                    path: candidate,
                    cleaned: false,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(OpenError::Io(error)),
        }
    }
}

pub(crate) fn create_gui_with(
    path: &Path,
    initialize: impl FnOnce(&Transaction<'_>) -> Result<(), OpenError>,
    before_publish: impl FnOnce() -> Result<(), OpenError>,
    open: impl FnOnce(&Path) -> Result<AppDb, OpenError>,
) -> Result<AppDb, OpenError> {
    create_gui_published(
        path,
        initialize,
        before_publish,
        open,
        |existing, new| fs::hard_link(existing, new),
        |_| Ok(()),
    )
}

/// Creates and publishes a database; `link` and `hook` let tests choose the route and interrupt it.
pub(crate) fn create_gui_published(
    path: &Path,
    initialize: impl FnOnce(&Transaction<'_>) -> Result<(), OpenError>,
    before_publish: impl FnOnce() -> Result<(), OpenError>,
    open: impl FnOnce(&Path) -> Result<AppDb, OpenError>,
    link: impl FnOnce(&Path, &Path) -> io::Result<()>,
    hook: impl FnMut(Step) -> Result<(), Interrupt>,
) -> Result<AppDb, OpenError> {
    let mut pending = reserve_temporary(path)?;
    let mut connection =
        Connection::open_with_flags(&pending.path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    initialize(&tx)?;
    tx.commit()?;
    drop(connection);
    before_publish()?;
    match link(&pending.path, path) {
        Ok(()) => {}
        Err(error) if link_unsupported(&error) => {
            if let Err(error) = publish_with_marker(&pending.path, path, hook) {
                // A kill leaves the temporary file as well.
                pending.cleaned = matches!(error, MarkerError::Crashed);
                return Err(match error {
                    MarkerError::Io(error) => OpenError::Io(error),
                    MarkerError::Crashed => OpenError::Io(io::Error::other("interrupted")),
                });
            }
        }
        Err(error) => return Err(OpenError::Io(error)),
    }
    fs::remove_file(&pending.path)
        .map_err(|error| OpenError::CreatedButOpenFailed(Box::new(OpenError::Io(error))))?;
    pending.cleaned = true;
    open(path).map_err(|error| OpenError::CreatedButOpenFailed(Box::new(error)))
}

/// Returns whether a hard link failed because the file system lacks the capability.
fn link_unsupported(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::Unsupported {
        return true;
    }
    #[cfg(windows)]
    const CODES: &[i32] = &[1, 50]; // ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED
    #[cfg(target_os = "linux")]
    const CODES: &[i32] = &[1, 38, 95]; // EPERM (FAT), ENOSYS, EOPNOTSUPP
    #[cfg(target_os = "macos")]
    const CODES: &[i32] = &[1, 45]; // EPERM, ENOTSUP
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    const CODES: &[i32] = &[];
    error
        .raw_os_error()
        .is_some_and(|code| CODES.contains(&code))
}

pub(crate) enum MarkerError {
    Io(io::Error),
    Crashed,
}

fn side(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn sentinel(path: &Path) -> PathBuf {
    side(path, ".creating")
}

fn marker(path: &Path) -> PathBuf {
    side(path, ".created")
}

fn step(hook: &mut impl FnMut(Step) -> Result<(), Interrupt>, at: Step) -> Result<(), MarkerError> {
    hook(at).map_err(|interrupt| match interrupt {
        Interrupt::Fail(error) => MarkerError::Io(error),
        Interrupt::Crash => MarkerError::Crashed,
    })
}

/// Runs the marker route. An I/O failure removes only what this call created; a crash
/// leaves everything for [`recover_incomplete_creation`].
pub(crate) fn publish_with_marker(
    temporary: &Path,
    target: &Path,
    mut hook: impl FnMut(Step) -> Result<(), Interrupt>,
) -> Result<(), MarkerError> {
    let sentinel_path = sentinel(target);
    let mut sentinel_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&sentinel_path)
        .map_err(MarkerError::Io)?;
    let mut created_database = false;
    let result = run_marker_steps(
        temporary,
        target,
        &mut sentinel_file,
        &mut created_database,
        &mut hook,
    );
    drop(sentinel_file);
    if let Err(MarkerError::Io(_)) = &result {
        if created_database {
            let _ = discard(target);
        } else {
            // Another creator owns the database name; remove only this call's sentinel.
            let _ = fs::remove_file(&sentinel_path);
        }
    }
    result
}

fn run_marker_steps(
    temporary: &Path,
    target: &Path,
    sentinel_file: &mut File,
    created_database: &mut bool,
    hook: &mut impl FnMut(Step) -> Result<(), Interrupt>,
) -> Result<(), MarkerError> {
    sentinel_file.sync_all().map_err(MarkerError::Io)?;
    sync_parent(target);
    step(hook, Step::SentinelWritten)?;
    let mut copy = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(MarkerError::Io)?;
    *created_database = true;
    step(hook, Step::DatabaseCreated)?;
    let length = io::copy(
        &mut File::open(temporary).map_err(MarkerError::Io)?,
        &mut copy,
    )
    .map_err(MarkerError::Io)?;
    copy.sync_all().map_err(MarkerError::Io)?;
    drop(copy);
    step(hook, Step::DatabaseCopied)?;
    let mut marker_file = File::create(marker(target)).map_err(MarkerError::Io)?;
    marker_file
        .write_all(format!("{length}\n").as_bytes())
        .and_then(|()| marker_file.sync_all())
        .map_err(MarkerError::Io)?;
    drop(marker_file);
    sync_parent(target);
    step(hook, Step::MarkerWritten)?;
    fs::remove_file(sentinel(target)).map_err(MarkerError::Io)?;
    sync_parent(target);
    step(hook, Step::SentinelRemoved)
}

/// Flushes the folder entry on Unix; Windows has no equivalent call in the standard library.
#[cfg(unix)]
fn sync_parent(path: &Path) {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = File::open(parent).and_then(|dir| dir.sync_all());
    }
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) {}

/// Returns whether the database at `path` has a valid completion marker for its current length.
fn marker_is_valid(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(marker(path)) else {
        return false;
    };
    let Ok(length) = fs::metadata(path).map(|metadata| metadata.len()) else {
        return false;
    };
    text.strip_suffix('\n')
        .and_then(|digits| digits.parse::<u64>().ok())
        == Some(length)
}

/// Returns whether a marker-route creation at `path` started but did not finish.
///
/// Read-only. Such a database must not be opened by any consumer.
pub(crate) fn is_incomplete(path: &Path) -> io::Result<bool> {
    Ok(sentinel(path).try_exists()? && !marker_is_valid(path))
}

/// Removes the database files left by an interrupted marker-route creation.
fn discard(path: &Path) -> io::Result<()> {
    for suffix in ["", "-wal", "-shm", "-journal", ".created"] {
        match fs::remove_file(side(path, suffix)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    // The sentinel goes last so an interrupted discard is repeated.
    match fs::remove_file(sentinel(path)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Repairs an interrupted marker-route creation before the database is opened or created.
///
/// Only the GUI startup path may call this, under the data folder's instance lock; it
/// deletes files. A database without a `.creating` file is never touched.
pub fn recover_incomplete_creation(path: &Path) -> io::Result<CreationRecovery> {
    if !sentinel(path).try_exists()? {
        return Ok(CreationRecovery::NotApplicable);
    }
    if marker_is_valid(path) {
        fs::remove_file(sentinel(path))?;
        return Ok(CreationRecovery::Completed);
    }
    discard(path)?;
    Ok(CreationRecovery::Discarded)
}

impl AppDb {
    /// See [`recover_incomplete_creation`].
    pub fn recover_incomplete_creation(path: &Path) -> io::Result<CreationRecovery> {
        recover_incomplete_creation(path)
    }
}
