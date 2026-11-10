//! Managed backup publication; deletion remains disabled without a safe OS port.
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use super::{Definitions, MigrationError, apply_next_with_backup, backup_snapshot, quick_check};
use crate::APPLICATION_ID;

const FORMAT: &str = "diskunfold-backup-v1";
const COPY: &str = "database.sqlite";
const MARKER: &str = "complete.json";
const MAX_MARKER_BYTES: usize = 4096;

/// Outcome of a migration whose backup was published in the managed directory.
#[derive(Debug)]
pub struct ManagedOutcome {
    pub revision: u32,
    pub backup_sequence: u64,
    pub retention: RetentionDiagnostic,
}

/// Retention candidates are reported without deleting any file.
#[derive(Debug, PartialEq, Eq)]
pub struct RetentionDiagnostic {
    pub content_valid: usize,
    pub recognized: usize,
    pub ignored: usize,
    pub older_than_three: usize,
    pub cleanup_unavailable: bool,
    pub inspection_failed: bool,
}

/// Applies a definition after publishing its complete managed backup.
///
/// The caller must hold the GUI instance lock and prevent replacement of the
/// database and backup directory. Cleanup is reported but never performed:
/// app-db lacks an identity-preserving deletion capability on Windows.
pub fn apply_next_managed(
    path: &Path,
    root: &Path,
    definitions: &Definitions<'_>,
) -> Result<ManagedOutcome, MigrationError> {
    apply_next_managed_with(path, root, definitions, inspect_retention)
}

fn apply_next_managed_with(
    path: &Path,
    root: &Path,
    definitions: &Definitions<'_>,
    inspect: impl FnOnce(&Path) -> Result<RetentionDiagnostic, MigrationError>,
) -> Result<ManagedOutcome, MigrationError> {
    check_root(root).map_err(backup_error)?;
    let mut sequence = None;
    let revision = apply_next_with_backup(
        path,
        definitions,
        |source, version| {
            sequence = Some(publish(source, version, root).map_err(backup_error)?);
            Ok(())
        },
        || Ok(()),
    )?;
    let backup_sequence = sequence.ok_or(MigrationError::Integrity)?;
    let retention = inspect(root).unwrap_or(RetentionDiagnostic {
        recognized: 0,
        content_valid: 0,
        ignored: 0,
        older_than_three: 0,
        cleanup_unavailable: true,
        inspection_failed: true,
    });
    Ok(ManagedOutcome {
        revision,
        backup_sequence,
        retention,
    })
}

fn backup_error(error: MigrationError) -> MigrationError {
    MigrationError::Backup(Box::new(error))
}

fn io(error: std::io::Error) -> MigrationError {
    MigrationError::Io(error)
}

fn check_root(root: &Path) -> Result<(), MigrationError> {
    let meta = fs::symlink_metadata(root).map_err(io)?;
    if !meta.is_dir() || is_link(&meta) {
        return Err(MigrationError::Integrity);
    }
    Ok(())
}

fn sequence_from(name: &str) -> Option<u64> {
    let rest = name
        .strip_prefix("backup-")
        .or_else(|| name.strip_prefix("pending-"))?;
    let digits = rest.get(..20)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let suffix = rest.get(20..)?;
    if !suffix.is_empty() && !suffix.starts_with('-') {
        return None;
    }
    digits.parse().ok()
}

fn next_sequence(root: &Path) -> Result<u64, MigrationError> {
    let mut max = 0u64;
    for entry in fs::read_dir(root).map_err(io)? {
        let entry = entry.map_err(io)?;
        if let Some(value) = entry.file_name().to_str().and_then(sequence_from) {
            max = max.max(value);
        }
    }
    max.checked_add(1).ok_or(MigrationError::Integrity)
}

fn publish(source: &Connection, version: u32, root: &Path) -> Result<u64, MigrationError> {
    let sequence = next_sequence(root)?;
    let staging = root.join(format!("pending-{sequence:020}-{}", uuid::Uuid::now_v7()));
    fs::create_dir(&staging).map_err(io)?;
    let copy = staging.join(COPY);
    backup_snapshot(source, &copy)?;
    let digest = hash_file(&copy)?;
    let marker =
        json!({"format":FORMAT,"sequence":sequence,"source_version":version,"blake3":digest});
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging.join(MARKER))
        .map_err(io)?;
    serde_json::to_writer(&mut file, &marker).map_err(|_| MigrationError::Integrity)?;
    file.flush().map_err(io)?;
    file.sync_all().map_err(io)?;
    drop(file);
    sync_dir(&staging)?;
    let final_path = root.join(format!("backup-{sequence:020}"));
    match fs::symlink_metadata(&final_path) {
        Ok(_) => return Err(MigrationError::Io(std::io::ErrorKind::AlreadyExists.into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io(error)),
    }
    fs::rename(&staging, &final_path).map_err(io)?;
    sync_dir(root)?;
    Ok(sequence)
}

fn hash_file(path: &Path) -> Result<String, MigrationError> {
    let mut file = fs::File::open(path).map_err(io)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Inspects complete owned copies; unknown and incomplete entries are preserved.
pub fn inspect_retention(root: &Path) -> Result<RetentionDiagnostic, MigrationError> {
    check_root(root)?;
    let mut valid = Vec::new();
    let mut content_valid = 0usize;
    let mut ignored = 0usize;
    for entry in fs::read_dir(root).map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = entry.file_name();
        let text = name.to_str().unwrap_or("");
        let Some(sequence) = text.strip_prefix("backup-").and_then(|v| {
            if v.len() == 20 && v.bytes().all(|b| b.is_ascii_digit()) {
                v.parse::<u64>().ok()
            } else {
                None
            }
        }) else {
            ignored += 1;
            continue;
        };
        if sequence == 0 {
            ignored += 1;
            continue;
        }
        if valid_entry(&entry.path(), sequence) {
            content_valid += 1;
            if link_counts_safe(&entry.path()) {
                valid.push(sequence);
            } else {
                ignored += 1;
            }
        } else {
            ignored += 1;
        }
    }
    valid.sort_unstable();
    Ok(RetentionDiagnostic {
        content_valid,
        recognized: valid.len(),
        ignored,
        older_than_three: valid.len().saturating_sub(3),
        cleanup_unavailable: true,
        inspection_failed: false,
    })
}

#[cfg(unix)]
fn link_counts_safe(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    [MARKER, COPY]
        .iter()
        .all(|name| fs::symlink_metadata(path.join(name)).is_ok_and(|meta| meta.nlink() == 1))
}

#[cfg(not(unix))]
fn link_counts_safe(_path: &Path) -> bool {
    false
}

fn valid_entry(path: &Path, sequence: u64) -> bool {
    validate_entry(path, sequence).is_ok()
}

fn validate_entry(path: &Path, sequence: u64) -> Result<(), MigrationError> {
    let meta = fs::symlink_metadata(path).map_err(io)?;
    if !meta.is_dir() || is_link(&meta) {
        return Err(MigrationError::Integrity);
    }
    let mut names = fs::read_dir(path)
        .map_err(io)?
        .map(|e| e.map(|x| x.file_name()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(io)?;
    names.sort();
    if names
        != [
            std::ffi::OsString::from(MARKER),
            std::ffi::OsString::from(COPY),
        ]
    {
        return Err(MigrationError::Integrity);
    }
    for name in [MARKER, COPY] {
        let meta = fs::symlink_metadata(path.join(name)).map_err(io)?;
        if !meta.is_file() || is_link(&meta) {
            return Err(MigrationError::Integrity);
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(path.join(MARKER))
        .map_err(io)?
        .take(MAX_MARKER_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > MAX_MARKER_BYTES {
        return Err(MigrationError::Integrity);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| MigrationError::Integrity)?;
    if serde_json::to_vec(&value).map_err(|_| MigrationError::Integrity)? != bytes {
        return Err(MigrationError::Integrity);
    }
    let obj = value.as_object().ok_or(MigrationError::Integrity)?;
    if obj.len() != 4
        || obj.get("format").and_then(Value::as_str) != Some(FORMAT)
        || obj.get("sequence").and_then(Value::as_u64) != Some(sequence)
    {
        return Err(MigrationError::Integrity);
    }
    let version = obj
        .get("source_version")
        .and_then(Value::as_u64)
        .ok_or(MigrationError::Integrity)?;
    let digest = obj
        .get("blake3")
        .and_then(Value::as_str)
        .ok_or(MigrationError::Integrity)?;
    let copy = path.join(COPY);
    if hash_file(&copy)? != digest {
        return Err(MigrationError::Integrity);
    }
    let mut header = [0u8; 20];
    fs::File::open(&copy)
        .map_err(io)?
        .read_exact(&mut header)
        .map_err(io)?;
    if &header[..16] != b"SQLite format 3\0" || header[18] != 1 || header[19] != 1 {
        return Err(MigrationError::Integrity);
    }
    let db = Connection::open_with_flags(copy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let id: u32 = db.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let actual: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if id != APPLICATION_ID || u64::from(actual) != version {
        return Err(MigrationError::Integrity);
    }
    quick_check(&db)
}

#[cfg(windows)]
fn is_link(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_type().is_symlink() || (meta.file_attributes() & 0x400 != 0)
}

#[cfg(not(windows))]
fn is_link(meta: &fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

fn sync_dir(path: &Path) -> Result<(), MigrationError> {
    #[cfg(unix)]
    {
        fs::File::open(path).map_err(io)?.sync_all().map_err(io)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod startup_tests;
#[cfg(test)]
mod tests;
