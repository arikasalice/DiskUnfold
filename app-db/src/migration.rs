//! One-step GUI database migrations over a caller-owned application database.
//!
//! The caller must hold the GUI instance lock and ensure that the database
//! path cannot be replaced during this operation.
use std::{
    fs::{self, OpenOptions},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use rusqlite::{
    Connection, OpenFlags, Transaction, TransactionBehavior,
    backup::{Backup, StepResult},
    hooks::{AuthAction, AuthContext, Authorization},
    params,
};

use crate::{APPLICATION_ID, BUSY_TIMEOUT, OpenError, reserve_temporary, settings};

pub mod managed;

const TRACKING_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS app_db_schema_meta (
 migration_no INTEGER PRIMARY KEY,
 definition_hash BLOB NOT NULL CHECK(length(definition_hash) = 32)
) STRICT;
CREATE TRIGGER IF NOT EXISTS app_db_schema_meta_no_update
BEFORE UPDATE ON app_db_schema_meta BEGIN SELECT RAISE(ABORT, 'append_only'); END;
CREATE TRIGGER IF NOT EXISTS app_db_schema_meta_no_delete
BEFORE DELETE ON app_db_schema_meta BEGIN SELECT RAISE(ABORT, 'append_only'); END;";

/// One globally numbered definition with LF line endings and no transaction control.
pub struct Definition<'a> {
    pub number: u32,
    pub sql: &'a str,
}

/// Definitions following an explicitly recognized existing baseline.
pub struct Definitions<'a> {
    pub baseline: u32,
    pub steps: &'a [Definition<'a>],
}

/// Migration failures leave the current definition unapplied.
#[derive(Debug)]
pub enum MigrationError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    Temporary(OpenError),
    Backup(Box<MigrationError>),
    ForeignDatabase,
    InvalidDefinitions,
    VersionMismatch,
    Tampered,
    Integrity,
    UnrecognizedBaseline,
    BackupBusy,
    /// A journal from an earlier unfinished migration exists; it must be recovered first.
    Interrupted,
    /// Restoring the pre-migration backup failed; the database and backups were kept.
    RestoreFailed(Box<MigrationError>),
}

impl From<rusqlite::Error> for MigrationError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// Backs up the fixed read snapshot and applies exactly the next definition.
///
/// The backup path must not already exist. The caller must provide a private
/// directory, serialize GUI startup, and prevent database-path replacement.
/// A failure after backup publication leaves that backup in place. This method
/// does not prune backups or start the GUI. Definitions are trusted built-in
/// SQL; arbitrary SQL and transaction-control statements are not supported.
pub fn apply_next(
    path: &Path,
    backup_path: &Path,
    definitions: &Definitions<'_>,
) -> Result<u32, MigrationError> {
    apply_next_with(path, backup_path, definitions, || Ok(()))
}

fn apply_next_with(
    path: &Path,
    backup_path: &Path,
    definitions: &Definitions<'_>,
    before_write: impl FnOnce() -> Result<(), MigrationError>,
) -> Result<u32, MigrationError> {
    apply_next_with_backup(
        path,
        definitions,
        |source, _| backup_snapshot(source, backup_path),
        before_write,
    )
}

fn apply_next_with_backup(
    path: &Path,
    definitions: &Definitions<'_>,
    backup: impl FnOnce(&Connection, u32) -> Result<(), MigrationError>,
    before_write: impl FnOnce() -> Result<(), MigrationError>,
) -> Result<u32, MigrationError> {
    validate_definitions(definitions)?;
    let mut source = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    source.busy_timeout(BUSY_TIMEOUT)?;
    source.pragma_update(None, "recursive_triggers", true)?;
    let definition_active = Arc::new(AtomicBool::new(false));
    let guard = Arc::clone(&definition_active);
    source.authorizer(Some(move |context: AuthContext<'_>| {
        if guard.load(Ordering::Relaxed) && prohibited(context.action) {
            Authorization::Deny
        } else {
            Authorization::Allow
        }
    }))?;
    let tx = source.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let id: u32 = tx.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if id != APPLICATION_ID {
        return Err(MigrationError::ForeignDatabase);
    }
    let current: u32 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let offset = applied_count(definitions, current)?;
    let next = definitions
        .steps
        .get(offset)
        .ok_or(MigrationError::VersionMismatch)?;
    check_history(&tx, definitions, offset)?;
    if offset == 0 {
        check_legacy_baseline(&tx)?;
    }
    quick_check(&tx)?;
    backup(&tx, current)?;
    before_write()?;
    tx.execute_batch(TRACKING_SCHEMA)?;
    definition_active.store(true, Ordering::Relaxed);
    let applied = tx.execute_batch(next.sql);
    definition_active.store(false, Ordering::Relaxed);
    applied?;
    record_applied(&tx, next)?;
    quick_check(&tx)?;
    tx.commit()?;
    Ok(next.number)
}

/// Applies built-in definitions while the database file is still unpublished.
pub(crate) fn initialize(
    tx: &Transaction<'_>,
    definitions: &Definitions<'_>,
) -> Result<(), MigrationError> {
    validate_definitions(definitions)?;
    tx.execute_batch(TRACKING_SCHEMA)?;
    for definition in definitions.steps {
        tx.execute_batch(definition.sql)?;
        record_applied(tx, definition)?;
    }
    quick_check(tx)
}

/// Checks applied definitions against one read snapshot before opening for use.
pub(crate) fn verify_registered(
    connection: &mut Connection,
    definitions: &Definitions<'_>,
    require_current: bool,
) -> Result<(), MigrationError> {
    validate_definitions(definitions)?;
    let tx = connection.transaction()?;
    verify_registered_snapshot(&tx, definitions, require_current, None, false)?;
    tx.commit()?;
    Ok(())
}

/// Verifies an unchanged database revision in one read-only transaction.
///
/// Unlike opening a registered database for normal use, a managed no-op may
/// accept the recognized legacy baseline. This method opens the connection
/// read-only; the caller must hold the database instance lock while checking
/// the revision.
pub(crate) fn verify_noop(
    path: &Path,
    definitions: &Definitions<'_>,
    expected_revision: u32,
) -> Result<(), MigrationError> {
    validate_definitions(definitions)?;
    applied_count(definitions, expected_revision)?;
    let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tx = connection.transaction()?;
    verify_registered_snapshot(&tx, definitions, false, Some(expected_revision), true)?;
    tx.commit()?;
    Ok(())
}

fn verify_registered_snapshot(
    tx: &Transaction<'_>,
    definitions: &Definitions<'_>,
    require_current: bool,
    expected_revision: Option<u32>,
    allow_baseline: bool,
) -> Result<(), MigrationError> {
    let id: u32 = tx.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if id != APPLICATION_ID {
        return Err(MigrationError::ForeignDatabase);
    }
    let version: u32 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if expected_revision.is_some_and(|expected| version != expected) {
        return Err(MigrationError::VersionMismatch);
    }
    let count = applied_count(definitions, version)?;
    if count == 0 {
        if !allow_baseline {
            return Err(MigrationError::VersionMismatch);
        }
        check_legacy_baseline(tx)?;
    } else {
        if require_current && count != definitions.steps.len() {
            return Err(MigrationError::VersionMismatch);
        }
        check_history(tx, definitions, count)?;
    }
    quick_check(tx)?;
    Ok(())
}

fn applied_count(definitions: &Definitions<'_>, version: u32) -> Result<usize, MigrationError> {
    if version == definitions.baseline {
        return Ok(0);
    }
    definitions
        .steps
        .binary_search_by_key(&version, |definition| definition.number)
        .map(|index| index + 1)
        .map_err(|_| MigrationError::VersionMismatch)
}

fn record_applied(tx: &Transaction<'_>, definition: &Definition<'_>) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO app_db_schema_meta(migration_no, definition_hash) VALUES (?1, ?2)",
        params![
            definition.number,
            blake3::hash(definition.sql.as_bytes())
                .as_bytes()
                .as_slice()
        ],
    )?;
    tx.pragma_update(None, "user_version", definition.number)
}

fn prohibited(action: AuthAction<'_>) -> bool {
    match action {
        AuthAction::Transaction { .. }
        | AuthAction::Savepoint { .. }
        | AuthAction::Pragma { .. }
        | AuthAction::Attach { .. }
        | AuthAction::Detach { .. } => true,
        AuthAction::Insert { table_name }
        | AuthAction::Delete { table_name }
        | AuthAction::Update { table_name, .. }
        | AuthAction::DropTable { table_name }
        | AuthAction::AlterTable { table_name, .. } => {
            table_name.eq_ignore_ascii_case("app_db_schema_meta")
        }
        AuthAction::DropTrigger { trigger_name, .. } => {
            trigger_name.eq_ignore_ascii_case("app_db_schema_meta_no_update")
                || trigger_name.eq_ignore_ascii_case("app_db_schema_meta_no_delete")
        }
        _ => false,
    }
}

fn validate_definitions(definitions: &Definitions<'_>) -> Result<(), MigrationError> {
    if definitions.baseline != 1 {
        return Err(MigrationError::InvalidDefinitions);
    }
    let mut previous = definitions.baseline;
    for definition in definitions.steps {
        if definition.number <= previous
            || definition.number > 9999
            || definition.sql.trim().is_empty()
            || definition.sql.contains('\r')
        {
            return Err(MigrationError::InvalidDefinitions);
        }
        previous = definition.number;
    }
    Ok(())
}

type SchemaObject = (String, String, String, Option<String>);

fn schema_objects(connection: &Connection) -> Result<Vec<SchemaObject>, MigrationError> {
    let mut statement = connection.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema
         WHERE name NOT GLOB 'sqlite_*' ORDER BY type, name",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn check_legacy_baseline(connection: &Connection) -> Result<(), MigrationError> {
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(settings::SCHEMA)?;
    if schema_objects(connection)? != schema_objects(&reference)? {
        return Err(MigrationError::UnrecognizedBaseline);
    }
    let mut stmt = connection.prepare(
        "SELECT version_key, settings_version, schema_version FROM user_env_setting_version",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let versions = rows.collect::<Result<Vec<_>, _>>()?;
    if versions.len() != 1 || versions[0].0 != "settings" || versions[0].1 < 0 || versions[0].2 < 1
    {
        return Err(MigrationError::UnrecognizedBaseline);
    }
    Ok(())
}

fn check_history(
    tx: &Transaction<'_>,
    definitions: &Definitions<'_>,
    offset: usize,
) -> Result<(), MigrationError> {
    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='app_db_schema_meta')", [], |row| row.get(0))?;
    if !exists {
        return if offset == 0 {
            Ok(())
        } else {
            Err(MigrationError::Tampered)
        };
    }
    let mut statement = tx.prepare(
        "SELECT migration_no, definition_hash FROM app_db_schema_meta ORDER BY migration_no",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    let mut count = 0;
    for row in rows {
        let (number, hash) = row?;
        let definition = definitions
            .steps
            .get(count)
            .ok_or(MigrationError::Tampered)?;
        if number != definition.number
            || hash.as_slice() != blake3::hash(definition.sql.as_bytes()).as_bytes()
        {
            return Err(MigrationError::Tampered);
        }
        count += 1;
    }
    if count != offset {
        return Err(MigrationError::Tampered);
    }
    Ok(())
}

fn quick_check(connection: &Connection) -> Result<(), MigrationError> {
    let mut stmt = connection.prepare("PRAGMA quick_check")?;
    let mut rows = stmt.query([])?;
    let first = rows.next()?.ok_or(MigrationError::Integrity)?;
    let result: String = first.get(0)?;
    if result != "ok" || rows.next()?.is_some() {
        return Err(MigrationError::Integrity);
    }
    Ok(())
}

fn backup_snapshot(source: &Connection, target: &Path) -> Result<(), MigrationError> {
    let mut pending = reserve_temporary(target).map_err(MigrationError::Temporary)?;
    let mut destination =
        Connection::open_with_flags(&pending.path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    {
        let backup = Backup::new(source, &mut destination)?;
        if !matches!(backup.step(-1)?, StepResult::Done) {
            return Err(MigrationError::BackupBusy);
        }
    }
    quick_check(&destination)?;
    let journal: String =
        destination.pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0))?;
    if !journal.eq_ignore_ascii_case("delete") {
        return Err(MigrationError::Integrity);
    }
    drop(destination);
    OpenOptions::new()
        .write(true)
        .open(&pending.path)
        .map_err(MigrationError::Io)?
        .sync_all()
        .map_err(MigrationError::Io)?;
    fs::hard_link(&pending.path, target).map_err(MigrationError::Io)?;
    fs::remove_file(&pending.path).map_err(MigrationError::Io)?;
    pending.cleaned = true;
    #[cfg(unix)]
    {
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::File::open(parent)
            .map_err(MigrationError::Io)?
            .sync_all()
            .map_err(MigrationError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
