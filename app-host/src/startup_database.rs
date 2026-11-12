//! Open the Windows GUI database under an instance lock before restoring settings.

use std::{fs, io, path::Path};

use du_app_db::{
    AppDb, HISTORY_DEFINITIONS, HISTORY_SCHEMA_VERSION, OpenError,
    migration::{
        MigrationError,
        managed::{RetentionDiagnostic, apply_managed_run},
    },
};
use du_platform_os::{
    application_data::DATABASE_FILE,
    windows_instance_lock::{InstanceLockError, WindowsInstanceLock},
    windows_observation::open_verified_stable,
};

use crate::shared_database::{DatabaseGuard, SharedDatabase};

/// Creates or migrates the database in the application's Windows data directory.
pub(super) fn open(data_dir: &Path) -> Result<SharedDatabase, io::Error> {
    let data_dir = std::path::absolute(data_dir)?;
    fs::create_dir_all(&data_dir)?;
    let instance = WindowsInstanceLock::acquire(&data_dir).map_err(lock_error)?;
    let path = data_dir.join(DATABASE_FILE);
    let (database, file) = if let Some(file) = instance.pin_database().map_err(lock_error)? {
        (open_existing(&path, &data_dir)?, file)
    } else {
        let database = AppDb::create_gui_with_history(&path).map_err(open_error)?;
        let file = instance
            .pin_database()
            .map_err(lock_error)?
            .ok_or_else(|| io::Error::other("db.open_failed: published database is missing"))?;
        (database, file)
    };
    Ok(SharedDatabase::new(
        database,
        Some(DatabaseGuard::new(file, instance)),
    ))
}

fn open_existing(path: &Path, data_dir: &Path) -> Result<AppDb, io::Error> {
    let version = AppDb::probe_schema_version_read_only(path).map_err(open_error)?;
    if version == HISTORY_SCHEMA_VERSION {
        return AppDb::open_gui_with_history_after_version_probe(path).map_err(open_error);
    }

    let reader = AppDb::open_read_only(path).map_err(open_error)?;
    let version = reader
        .schema_version()
        .map_err(|error| open_error(OpenError::Sqlite(error)))?;
    drop(reader);
    if version < HISTORY_SCHEMA_VERSION {
        migrate(path, &data_dir.join("db-backups"), version)?;
    }
    AppDb::open_gui_with_history(path).map_err(open_error)
}

fn migrate(path: &Path, backups: &Path, version: u32) -> Result<(), io::Error> {
    fs::create_dir_all(backups)
        .map_err(|error| io::Error::other(format!("db.backup_failed: {error}")))?;
    let _directory = open_verified_stable(backups)
        .map_err(|error| io::Error::other(format!("db.backup_failed: {error:?}")))?;
    let outcome = match apply_managed_run(path, backups, &HISTORY_DEFINITIONS, version) {
        Ok(outcome) => outcome,
        Err(failure) => {
            report_retention(&failure.retention);
            return Err(migration_error(failure.migration));
        }
    };
    report_retention(&outcome.retention);
    if outcome.revision != HISTORY_SCHEMA_VERSION {
        return Err(io::Error::other("db.schema_mismatch"));
    }
    Ok(())
}

fn report_retention(retention: &RetentionDiagnostic) {
    if retention.inspection_failed || (retention.cleanup_unavailable && retention.content_valid > 3)
    {
        eprintln!("db.backup_retention_unavailable");
    }
}

fn lock_error(error: InstanceLockError) -> io::Error {
    match error {
        InstanceLockError::Busy => io::Error::new(
            io::ErrorKind::AlreadyExists,
            "DiskUnfold is already running for this data directory",
        ),
        error => io::Error::other(format!("db.open_failed: {error:?}")),
    }
}

fn open_error(error: OpenError) -> io::Error {
    match error {
        OpenError::UnsupportedSchema(_) => io::Error::other("db.schema_mismatch"),
        OpenError::HistorySchema(error) => migration_error(*error),
        error => io::Error::other(format!("db.open_failed: {error:?}")),
    }
}

fn migration_error(error: MigrationError) -> io::Error {
    let key = match error {
        MigrationError::VersionMismatch | MigrationError::UnrecognizedBaseline => {
            "db.schema_mismatch"
        }
        MigrationError::Tampered => "db.migration_tampered",
        MigrationError::Backup(_) => "db.backup_failed",
        _ => "db.open_failed",
    };
    io::Error::other(format!("{key}: {error:?}"))
}

#[cfg(test)]
mod tests;
