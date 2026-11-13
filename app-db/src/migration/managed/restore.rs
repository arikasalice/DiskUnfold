//! Interrupted-migration journal and full restore from the pre-migration backup.
//!
//! F14 DLD rev79 §2.5.5 keeps the steps a failed migration already committed and resumes on the
//! next start. F15 HLD rev116 §2.3.3.5 and DLD rev70 §2.6 (FR8.HD1.app-db.DD1) require a full
//! restore of the pre-migration backup instead. The user decided to follow F15, so this module
//! implements the restore and F14 DLD79 §2.5.5 needs a design revision (see
//! docs/handoff/F14-DLD79-2.5.5-needs-revision.md).
//!
//! The journal file in the backup directory is the durable record of a migration run. It names the
//! pre-migration backup, which stays the source of truth. The backup is restored in place through
//! SQLite's online backup API, so the destination changes in one transaction, the database file
//! keeps its identity (the pinned handle and the macOS path-identity checks stay valid), and the
//! WAL and SHM sidecars are reconciled by SQLite and checkpointed to an empty log.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
    types::ValueRef,
};
use serde_json::{Value, json};

use super::{COPY, check_root, hash_file, io, sync_dir, validate_entry};
use crate::{
    APPLICATION_ID, BUSY_TIMEOUT,
    migration::{self, Definitions, MigrationError},
};

/// File name of the journal inside the managed backup directory.
pub const JOURNAL: &str = "migration-journal.json";
const JOURNAL_TEMP: &str = "migration-journal.json.tmp";
const JOURNAL_FORMAT: &str = "diskunfold-migration-journal-v1";
const MAX_JOURNAL_BYTES: u64 = 4096;

/// Backup that was restored over the live database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoredBackup {
    /// Sequence of the managed backup used as the source of truth.
    pub sequence: u64,
    /// Schema revision the restored database reports.
    pub version: u32,
}

/// Outcome of the restore attempt that follows a failed migration run.
#[derive(Debug)]
pub enum RestoreStatus {
    /// The failure happened before the database could change, so nothing was restored.
    NotNeeded,
    /// The verified backup replaced the live database and the result was checked.
    Restored(RestoredBackup),
    /// Restoring failed. The database, backups and journal are all preserved.
    Failed(MigrationError),
}

/// Result of finishing a migration or restore that a previous process left unfinished.
#[derive(Debug, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// No journal exists, so no migration was interrupted.
    NothingPending,
    /// The interrupted migration had committed its last step; the journal was cleared.
    AlreadyComplete {
        /// Revision the database reports.
        version: u32,
    },
    /// The pre-migration backup was restored and verified.
    Restored(RestoredBackup),
}

/// Points of the restore at which tests inject a failure or a simulated process stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreStage {
    /// Recovery is about to decide whether an open journal's migration already completed.
    Deciding,
    /// The journal says restoring; the backup has not been read yet.
    Begun,
    /// The backup content was written to the live database; it is not yet verified.
    Copied,
    /// The restored database passed verification; the journal is not yet finalized.
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Migrating,
    Restoring,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            Self::Migrating => "migrating",
            Self::Restoring => "restoring",
        }
    }
}

/// Durable description of one migration run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Journal {
    state: State,
    sequence: u64,
    blake3: String,
    source_version: u32,
    target_version: u32,
    failure: Option<Failure>,
}

/// What made the run restore: the step that was being applied and the error category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Failure {
    /// Number of the definition that was being applied, when the failure belongs to one.
    pub(super) step: Option<u32>,
    pub(super) category: String,
}

impl Failure {
    pub(super) fn new(step: Option<u32>, error: &MigrationError) -> Self {
        Self {
            step,
            category: category(error).to_owned(),
        }
    }
}

/// Stable, content-free category of a migration error for the restore record.
fn category(error: &MigrationError) -> &'static str {
    match error {
        MigrationError::Sqlite(_) => "sqlite",
        MigrationError::Io(_) => "io",
        MigrationError::Temporary(_) => "temporary",
        MigrationError::Backup(inner) => category(inner),
        MigrationError::ForeignDatabase => "foreign_database",
        MigrationError::InvalidDefinitions => "invalid_definitions",
        MigrationError::VersionMismatch => "version_mismatch",
        MigrationError::Tampered => "tampered",
        MigrationError::Integrity => "integrity",
        MigrationError::UnrecognizedBaseline => "unrecognized_baseline",
        MigrationError::BackupBusy => "busy",
        MigrationError::Interrupted => "interrupted",
        MigrationError::RestoreFailed(_) => "restore_failed",
    }
}

impl Journal {
    fn to_value(&self) -> Value {
        let state = self.state.name();
        let mut value = json!({
            "format": JOURNAL_FORMAT,
            "state": state,
            "backup_sequence": self.sequence,
            "blake3": self.blake3,
            "source_version": self.source_version,
            "target_version": self.target_version,
        });
        if let (Some(failure), Some(object)) = (&self.failure, value.as_object_mut()) {
            object.insert(
                "failure".to_owned(),
                json!({"step": failure.step, "category": failure.category}),
            );
        }
        value
    }

    fn parse(bytes: &[u8]) -> Result<Self, MigrationError> {
        let value: Value = serde_json::from_slice(bytes).map_err(|_| MigrationError::Integrity)?;
        let object = value.as_object().ok_or(MigrationError::Integrity)?;
        let number = |key: &str| object.get(key).and_then(Value::as_u64);
        let state = match object.get("state").and_then(Value::as_str) {
            Some("migrating") => State::Migrating,
            Some("restoring") => State::Restoring,
            _ => return Err(MigrationError::Integrity),
        };
        let failure = match object.get("failure") {
            None => None,
            Some(Value::Object(record)) if record.len() == 2 => {
                let step = match record.get("step") {
                    Some(Value::Null) => None,
                    Some(value) => Some(
                        value
                            .as_u64()
                            .and_then(|v| u32::try_from(v).ok())
                            .ok_or(MigrationError::Integrity)?,
                    ),
                    None => return Err(MigrationError::Integrity),
                };
                let category = record
                    .get("category")
                    .and_then(Value::as_str)
                    .ok_or(MigrationError::Integrity)?;
                Some(Failure {
                    step,
                    category: category.to_owned(),
                })
            }
            Some(_) => return Err(MigrationError::Integrity),
        };
        let valid = object.len() == 6 + usize::from(failure.is_some())
            && object.get("format").and_then(Value::as_str) == Some(JOURNAL_FORMAT);
        let sequence = number("backup_sequence");
        let source = number("source_version").and_then(|v| u32::try_from(v).ok());
        let target = number("target_version").and_then(|v| u32::try_from(v).ok());
        let blake3 = object.get("blake3").and_then(Value::as_str);
        match (valid, sequence, source, target, blake3) {
            (true, Some(sequence), Some(source_version), Some(target_version), Some(blake3))
                if sequence > 0 && source_version < target_version =>
            {
                Ok(Self {
                    state,
                    sequence,
                    blake3: blake3.to_owned(),
                    source_version,
                    target_version,
                    failure,
                })
            }
            _ => Err(MigrationError::Integrity),
        }
    }

    fn directory(&self, root: &Path) -> PathBuf {
        root.join(format!("backup-{:020}", self.sequence))
    }
}

fn write_journal(root: &Path, journal: &Journal) -> Result<(), MigrationError> {
    let temp = root.join(JOURNAL_TEMP);
    // Only a regular file or a plain symlink may be a stale temp file; either is removed by
    // name, never followed. Anything else is unexpected and blocks the write.
    match fs::symlink_metadata(&temp) {
        Ok(meta) if meta.is_file() || meta.file_type().is_symlink() => {
            fs::remove_file(&temp).map_err(io)?;
        }
        Ok(_) => return Err(MigrationError::Integrity),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io(error)),
    }
    // create_new fails on any existing name, including a link that appeared after the check.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(io)?;
    serde_json::to_writer(&mut file, &journal.to_value()).map_err(|_| MigrationError::Integrity)?;
    file.flush().map_err(io)?;
    file.sync_all().map_err(io)?;
    drop(file);
    fs::rename(&temp, root.join(JOURNAL)).map_err(io)?;
    sync_dir(root)
}

/// Reads the journal; an unreadable or malformed one is an error that blocks startup.
pub(super) fn read_journal(root: &Path) -> Result<Option<Journal>, MigrationError> {
    let path = root.join(JOURNAL);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(error)),
    };
    if !meta.is_file() || meta.len() > MAX_JOURNAL_BYTES {
        return Err(MigrationError::Integrity);
    }
    Journal::parse(&fs::read(path).map_err(io)?).map(Some)
}

/// Verifies the just-published pre-migration backup and records it before the first write.
pub(super) fn begin_journal(
    root: &Path,
    sequence: u64,
    source_version: u32,
    target_version: u32,
) -> Result<(), MigrationError> {
    if read_journal(root)?.is_some() {
        return Err(MigrationError::Interrupted);
    }
    let directory = root.join(format!("backup-{sequence:020}"));
    validate_entry(&directory, sequence)?;
    let journal = Journal {
        state: State::Migrating,
        sequence,
        blake3: hash_file(&directory.join(COPY))?,
        source_version,
        target_version,
        failure: None,
    };
    write_journal(root, &journal)
}

/// Removes the journal after a migration run committed every step.
pub(super) fn clear_journal(root: &Path) {
    // A leftover journal is harmless: the next start sees the target revision and clears it.
    if fs::remove_file(root.join(JOURNAL)).is_ok() {
        let _ = sync_dir(root);
    }
}

pub(super) type Fault<'a> = &'a mut dyn FnMut(RestoreStage) -> Result<(), MigrationError>;

/// Restores the backup named by the journal, if any, after a failed run.
pub(super) fn restore_after_failure(
    path: &Path,
    root: &Path,
    failure: Failure,
    fault: Fault<'_>,
) -> RestoreStatus {
    match read_journal(root) {
        Ok(None) => RestoreStatus::NotNeeded,
        Ok(Some(journal)) => {
            let journal = Journal {
                failure: journal.failure.or(Some(failure)),
                ..journal
            };
            match finish_restore(path, root, &journal, fault) {
                Ok(restored) => RestoreStatus::Restored(restored),
                Err(error) => RestoreStatus::Failed(error),
            }
        }
        Err(error) => RestoreStatus::Failed(error),
    }
}

/// Finishes a migration or restore that an earlier process left unfinished.
///
/// The caller must hold the GUI instance lock with no other handle writing the database. A
/// journal in the migrating state whose database reports exactly the target revision and passes
/// the integrity check is cleared. A database at or above the target revision is never
/// restored: a failed check there is reported and the journal kept. A migrating journal whose
/// database reports a lower revision, and every restoring journal, restores the pre-migration
/// backup, so a repeated call after a stop at any point of the restore reaches the same result.
/// Any error while reading the revision or checking integrity (a busy database, an I/O error)
/// fails closed. A failure keeps the database, the backups and the journal and is reported as
/// [MigrationError::RestoreFailed].
pub fn recover_interrupted(
    path: &Path,
    root: &Path,
    definitions: &Definitions<'_>,
) -> Result<RecoveryOutcome, MigrationError> {
    recover_with(path, root, definitions, &mut |_| Ok(()))
}

pub(super) fn recover_with(
    path: &Path,
    root: &Path,
    definitions: &Definitions<'_>,
    fault: Fault<'_>,
) -> Result<RecoveryOutcome, MigrationError> {
    let failed = |error| MigrationError::RestoreFailed(Box::new(error));
    let Some(journal) = read_journal(root).map_err(failed)? else {
        return Ok(RecoveryOutcome::NothingPending);
    };
    let mut journal = journal;
    if journal.state == State::Migrating {
        fault(RestoreStage::Deciding).map_err(failed)?;
        let revision = committed_revision(path).map_err(failed)?;
        if revision >= journal.target_version {
            // Never discard data the database already committed at or past the target.
            complete_check(path, definitions, &journal, revision).map_err(failed)?;
            clear_journal(root);
            return Ok(RecoveryOutcome::AlreadyComplete {
                version: journal.target_version,
            });
        }
        // A definite mismatch: the last step never committed. Record where it stopped.
        journal.failure = journal.failure.or(Some(Failure {
            step: revision.checked_add(1),
            category: "interrupted".to_owned(),
        }));
    }
    finish_restore(path, root, &journal, fault)
        .map(RecoveryOutcome::Restored)
        .map_err(failed)
}

/// Reads the committed schema revision of the live database through a read-only connection.
fn committed_revision(path: &Path) -> Result<u32, MigrationError> {
    let live = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    live.busy_timeout(BUSY_TIMEOUT)?;
    let id: u32 = live.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if id != APPLICATION_ID {
        return Err(MigrationError::ForeignDatabase);
    }
    user_version(&live)
}

/// Accepts a database at the target revision whose integrity check passes.
fn complete_check(
    path: &Path,
    definitions: &Definitions<'_>,
    journal: &Journal,
    revision: u32,
) -> Result<(), MigrationError> {
    if revision != journal.target_version {
        // Newer than this journal knows; neither complete nor restorable.
        return Err(MigrationError::VersionMismatch);
    }
    migration::verify_noop(path, definitions, revision)
}

fn finish_restore(
    path: &Path,
    root: &Path,
    journal: &Journal,
    fault: Fault<'_>,
) -> Result<RestoredBackup, MigrationError> {
    check_root(root)?;
    if journal.state == State::Migrating {
        let restoring = Journal {
            state: State::Restoring,
            ..journal.clone()
        };
        write_journal(root, &restoring)?;
    }
    fault(RestoreStage::Begun)?;
    let directory = journal.directory(root);
    validate_entry(&directory, journal.sequence)?;
    let copy = directory.join(COPY);
    if hash_file(&copy)? != journal.blake3 {
        return Err(MigrationError::Integrity);
    }
    let backup = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    if user_version(&backup)? != journal.source_version {
        return Err(MigrationError::Integrity);
    }
    let expected = logical_digest(&backup)?;
    copy_over(path, &backup)?;
    drop(backup);
    fault(RestoreStage::Copied)?;
    verify_restored(path, journal.source_version, &expected)?;
    fault(RestoreStage::Verified)?;
    // The journal stays under a new name as the record of the restore.
    let finished = root.join(format!("restored-{:020}.json", journal.sequence));
    fs::rename(root.join(JOURNAL), finished).map_err(io)?;
    sync_dir(root)?;
    Ok(RestoredBackup {
        sequence: journal.sequence,
        version: journal.source_version,
    })
}

/// Writes every page of the backup into the existing live database in one transaction.
fn copy_over(path: &Path, backup: &Connection) -> Result<(), MigrationError> {
    let mut live = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(busy_or_sqlite)?;
    live.busy_timeout(BUSY_TIMEOUT)?;
    {
        let copy = Backup::new(backup, &mut live).map_err(busy_or_sqlite)?;
        if !matches!(copy.step(-1).map_err(busy_or_sqlite)?, StepResult::Done) {
            return Err(MigrationError::BackupBusy);
        }
    }
    // Move the restored pages out of the log so the sidecars end empty.
    let busy: i64 = live
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(busy_or_sqlite)?;
    if busy != 0 {
        return Err(MigrationError::BackupBusy);
    }
    live.close()
        .map_err(|(_, error)| MigrationError::Sqlite(error))
}

/// A lock held by another connection is a retryable condition, not a damaged database.
fn busy_or_sqlite(error: rusqlite::Error) -> MigrationError {
    if matches!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    ) {
        MigrationError::BackupBusy
    } else {
        MigrationError::Sqlite(error)
    }
}

fn verify_restored(path: &Path, version: u32, expected: &str) -> Result<(), MigrationError> {
    {
        let live = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        live.busy_timeout(BUSY_TIMEOUT)?;
        let id: u32 = live.pragma_query_value(None, "application_id", |row| row.get(0))?;
        if id != APPLICATION_ID || user_version(&live)? != version {
            return Err(MigrationError::Integrity);
        }
        let mut statement = live.prepare("PRAGMA integrity_check")?;
        let mut rows = statement.query([])?;
        let first: String = rows.next()?.ok_or(MigrationError::Integrity)?.get(0)?;
        if first != "ok" || rows.next()?.is_some() {
            return Err(MigrationError::Integrity);
        }
        if logical_digest(&live)? != expected {
            return Err(MigrationError::Integrity);
        }
    }
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    match fs::metadata(PathBuf::from(wal)) {
        Ok(meta) if meta.len() > 0 => Err(MigrationError::Integrity),
        _ => Ok(()),
    }
}

fn user_version(connection: &Connection) -> Result<u32, MigrationError> {
    Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Hashes the schema text and every row of every table in scan order.
///
/// Two databases with the same digest hold the same schema and the same rows.
pub(super) fn logical_digest(connection: &Connection) -> Result<String, MigrationError> {
    let mut hasher = blake3::Hasher::new();
    let mut tables = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT type, name, tbl_name, coalesce(sql, '') FROM sqlite_schema ORDER BY type, name",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            for index in 0..4 {
                let text: String = row.get(index)?;
                hasher.update(&(text.len() as u64).to_le_bytes());
                hasher.update(text.as_bytes());
            }
            if row.get::<_, String>(0)? == "table" {
                tables.push(row.get::<_, String>(1)?);
            }
        }
    }
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let mut statement = connection.prepare(&format!("SELECT * FROM \"{quoted}\""))?;
        let columns = statement.column_count();
        hasher.update(table.as_bytes());
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            hasher.update(b"R");
            for index in 0..columns {
                match row.get_ref(index)? {
                    ValueRef::Null => hasher.update(&[0]),
                    ValueRef::Integer(v) => hasher.update(&[1]).update(&v.to_le_bytes()),
                    ValueRef::Real(v) => hasher.update(&[2]).update(&v.to_bits().to_le_bytes()),
                    ValueRef::Text(b) => hasher
                        .update(&[3])
                        .update(&(b.len() as u64).to_le_bytes())
                        .update(b),
                    ValueRef::Blob(b) => hasher
                        .update(&[4])
                        .update(&(b.len() as u64).to_le_bytes())
                        .update(b),
                };
            }
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}
