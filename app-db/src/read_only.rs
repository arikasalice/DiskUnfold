//! Read-only connections that never create, repair or modify database files.
//!
//! SQLite opens a WAL-mode database through `-wal` and `-shm` side files and
//! creates them even for `SQLITE_OPEN_READONLY`. This module decides, from the
//! file system alone, which of three outcomes applies before SQLite sees the path:
//!
//! * The database is not in WAL mode: a plain read-only connection.
//! * The database is in WAL mode and both `-wal` and `-shm` exist (a running
//!   writer, normally the GUI): a live read-only connection that follows the
//!   writer's commits. It shares the writer's `-shm` locks and read marks.
//! * The database is in WAL mode, there is no `-wal`/`-shm` pair and the log
//!   is absent or empty (the writer closed cleanly, so the main file holds every
//!   committed page): a private in-memory snapshot of the main file. It never
//!   changes afterwards.
//!
//! A WAL database that has log frames but no `-shm` (a writer that crashed) needs
//! recovery, which writes files. That state is `OpenError::ReadOnlyUnavailable`.

use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags};

use crate::OpenError;

/// Largest main file copied into a private snapshot.
const MAX_SNAPSHOT_BYTES: u64 = 256 * 1024 * 1024;
/// Length of the SQLite file header.
const HEADER_LEN: usize = 100;

/// Opens `path` for reading without creating or modifying any file.
pub(crate) fn open_connection(path: &Path) -> Result<Connection, OpenError> {
    if !has_wal_header(path) {
        return plain(path);
    }
    let wal = side_file(path, "-wal");
    let shm = side_file(path, "-shm");
    match (file_len(&wal)?, file_len(&shm)?) {
        (Some(_), Some(_)) => plain(path),
        (Some(frames), None) if frames > 0 => Err(OpenError::ReadOnlyUnavailable),
        _ => snapshot(path, &wal),
    }
}

fn plain(path: &Path) -> Result<Connection, OpenError> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?)
}

/// Returns `<path><suffix>`, the name SQLite gives a side file.
fn side_file(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Length of a side file, or `None` when it does not exist.
fn file_len(path: &Path) -> Result<Option<u64>, OpenError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(OpenError::Io(error)),
    }
}

/// Whether the header read/write versions (bytes 18 and 19) both name WAL.
/// Anything unreadable is left to SQLite, which reports its usual error.
fn has_wal_header(path: &Path) -> bool {
    read_header(path).is_ok_and(|header| header[18] == 2 && header[19] == 2)
}

fn read_header(path: &Path) -> io::Result<[u8; HEADER_LEN]> {
    let mut header = [0_u8; HEADER_LEN];
    File::open(path)?.read_exact(&mut header)?;
    Ok(header)
}

/// Copies the main file of a WAL database without log frames into memory.
///
/// The copy is accepted only when no writer was visible before, during and after
/// it: the log stays empty or absent, and the length and header are unchanged and
/// describe whole pages. The in-memory copy is marked as a rollback-journal database, so
/// SQLite never looks for side files.
fn snapshot(path: &Path, wal: &Path) -> Result<Connection, OpenError> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    if length > MAX_SNAPSHOT_BYTES {
        return Err(OpenError::ReadOnlyUnavailable);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
    file.read_to_end(&mut bytes)?;
    let quiet = file_len(wal)?.is_none_or(|frames| frames == 0);
    let page_size = match u16::from_be_bytes([
        bytes.get(16).copied().unwrap_or(0),
        bytes.get(17).copied().unwrap_or(0),
    ]) {
        1 => 65_536_u64,
        size => u64::from(size),
    };
    let stable = quiet
        && bytes.len() as u64 == length
        && bytes.len() >= HEADER_LEN
        && page_size >= 512
        && length % page_size == 0
        && file.metadata()?.len() == length
        && read_header(path)?[..] == bytes[..HEADER_LEN];
    if !stable {
        return Err(OpenError::ReadOnlyUnavailable);
    }
    bytes[18] = 1;
    bytes[19] = 1;
    let mut connection = Connection::open_in_memory()?;
    connection.deserialize_read_exact(
        rusqlite::MAIN_DB,
        io::Cursor::new(&bytes),
        bytes.len(),
        true,
    )?;
    Ok(connection)
}
