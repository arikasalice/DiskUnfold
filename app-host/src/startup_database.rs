//! Open the GUI database under a platform instance lock before restoring settings.

use std::{fs, io, path::Path};

use du_app_db::{AppDb, HISTORY_SCHEMA_VERSION, OpenError, migration::MigrationError};
#[cfg(any(windows, target_os = "linux"))]
use du_app_db::{
    HISTORY_DEFINITIONS,
    migration::managed::{RetentionDiagnostic, apply_managed_run},
};
#[cfg(target_os = "linux")]
use du_platform_os::{
    application_data::DATABASE_FILE,
    linux_instance_lock::{LinuxInstanceLock, LinuxInstanceLockError},
};
#[cfg(target_os = "macos")]
use du_platform_os::{
    application_data::DATABASE_FILE,
    macos_instance_lock::{MacOsInstanceLock, MacOsInstanceLockError},
};
#[cfg(windows)]
use du_platform_os::{
    application_data::DATABASE_FILE,
    windows_instance_lock::{InstanceLockError, WindowsInstanceLock},
    windows_observation::open_verified_stable,
};

use crate::shared_database::{DatabaseGuard, SharedDatabase};

/// Database startup result plus whether this run created the database file.
pub(super) struct OpenedDatabase {
    pub(super) database: SharedDatabase,
    pub(super) first_run: bool,
}

/// Adds the HLD caller-facing startup reason key to a data-directory failure.
pub(super) fn data_folder_error(error: Option<io::Error>) -> io::Error {
    // Select the HLD 116 caller-facing key here; it conflicts with DLD 70's
    // startup.data_folder_unavailable key and no internal DLD key is produced.
    match error {
        Some(error) => io::Error::other(format!("startup.data_dir_unwritable: {error}")),
        None => io::Error::other("startup.data_dir_unwritable"),
    }
}

/// Creates or migrates the database in the application's Windows data directory.
#[cfg(all(windows, test))]
pub(super) fn open(data_dir: &Path) -> Result<SharedDatabase, io::Error> {
    open_with_status(data_dir).map(|opened| opened.database)
}

/// Opens the Windows startup database and retains the creator's publication outcome.
#[cfg(windows)]
pub(super) fn open_with_status(data_dir: &Path) -> Result<OpenedDatabase, io::Error> {
    let data_dir = std::path::absolute(data_dir).map_err(|error| data_folder_error(Some(error)))?;
    create_data_directory(&data_dir).map_err(|error| data_folder_error(Some(error)))?;
    let instance = WindowsInstanceLock::acquire(&data_dir).map_err(lock_error)?;
    let path = data_dir.join(DATABASE_FILE);
    let (database, file, first_run) =
        if let Some(file) = instance.pin_database().map_err(lock_error)? {
            (open_existing(&path, &data_dir)?, file, false)
        } else {
            let database = AppDb::create_gui_with_history(&path).map_err(open_error)?;
            let file = instance
                .pin_database()
                .map_err(lock_error)?
                .ok_or_else(|| io::Error::other("db.open_failed: published database is missing"))?;
            (database, file, true)
        };
    Ok(OpenedDatabase {
        database: SharedDatabase::new(database, Some(DatabaseGuard::new(file, instance))),
        first_run,
    })
}

/// Opens or atomically creates the current macOS GUI database under its per-directory lock.
///
/// Existing lower-format databases are not migrated while the published F14/F15 migration
/// failure contract conflict remains unresolved. The SQLite read-only probe may initialize WAL
/// sidecars; it preserves the main database and existing WAL frames and leaves sidecars under
/// SQLite's normal locking and recovery behavior.
#[cfg(target_os = "macos")]
pub(super) fn open_with_status(data_dir: &Path) -> Result<OpenedDatabase, io::Error> {
    let data_dir = std::path::absolute(data_dir).map_err(|error| data_folder_error(Some(error)))?;
    create_data_directory(&data_dir).map_err(|error| data_folder_error(Some(error)))?;
    let instance = MacOsInstanceLock::acquire(&data_dir).map_err(macos_lock_error)?;
    verify_macos_paths(&instance, &data_dir, None)?;
    let path = data_dir.join(DATABASE_FILE);
    let (database, file, first_run) = match instance.pin_database().map_err(macos_lock_error)? {
        Some(file) => {
            verify_macos_paths(&instance, &data_dir, Some(&file))?;
            let database = open_existing_macos(&path, &data_dir, &instance, &file)?;
            (database, file, false)
        }
        None => {
            let database = AppDb::create_gui_with_history(&path).map_err(open_error)?;
            verify_macos_paths(&instance, &data_dir, None)?;
            let file = instance
                .pin_database()
                .map_err(macos_lock_error)?
                .ok_or_else(|| io::Error::other("db.open_failed: published database is missing"))?;
            verify_macos_paths(&instance, &data_dir, Some(&file))?;
            (database, file, true)
        }
    };
    Ok(OpenedDatabase {
        database: SharedDatabase::new(database, Some(DatabaseGuard::new(file, instance))),
        first_run,
    })
}

#[cfg(all(test, target_os = "macos"))]
pub(super) fn open(data_dir: &Path) -> Result<SharedDatabase, io::Error> {
    open_with_status(data_dir).map(|opened| opened.database)
}

#[cfg(target_os = "macos")]
fn open_existing_macos(
    path: &Path,
    data_dir: &Path,
    instance: &MacOsInstanceLock,
    pinned_file: &fs::File,
) -> Result<AppDb, io::Error> {
    verify_macos_paths(instance, data_dir, Some(pinned_file))?;
    let version = AppDb::probe_schema_version_read_only(path).map_err(open_error)?;
    verify_macos_paths(instance, data_dir, Some(pinned_file))?;
    if version < HISTORY_SCHEMA_VERSION {
        return Err(io::Error::other(format!(
            "startup.migration_failed: database schema {version} is preserved; migration awaits the published F14/F15 policy resolution"
        )));
    }
    if version > HISTORY_SCHEMA_VERSION {
        return Err(io::Error::other("db.schema_mismatch"));
    }
    let database = AppDb::open_gui_with_history_after_version_probe(path).map_err(open_error)?;
    verify_macos_paths(instance, data_dir, Some(pinned_file))?;
    Ok(database)
}

/// Creates or migrates the database in the application's Linux data directory.
#[cfg(all(target_os = "linux", test))]
pub(super) fn open(data_dir: &Path) -> Result<SharedDatabase, io::Error> {
    open_with_status(data_dir).map(|opened| opened.database)
}

/// Opens the Linux startup database and retains the creator's publication outcome.
#[cfg(target_os = "linux")]
pub(super) fn open_with_status(data_dir: &Path) -> Result<OpenedDatabase, io::Error> {
    let data_dir = std::path::absolute(data_dir).map_err(|error| data_folder_error(Some(error)))?;
    create_data_directory(&data_dir).map_err(|error| data_folder_error(Some(error)))?;
    let instance = LinuxInstanceLock::acquire(&data_dir).map_err(linux_lock_error)?;
    let path = data_dir.join(DATABASE_FILE);
    let (database, file, first_run) =
        if let Some(file) = instance.pin_database().map_err(linux_lock_error)? {
            (open_existing(&path, &data_dir, &instance)?, file, false)
        } else {
            let database = AppDb::create_gui_with_history(&path).map_err(open_error)?;
            let file = instance
                .pin_database()
                .map_err(linux_lock_error)?
                .ok_or_else(|| io::Error::other("db.open_failed: published database is missing"))?;
            (database, file, true)
        };
    Ok(OpenedDatabase {
        database: SharedDatabase::new(database, Some(DatabaseGuard::new(file, instance))),
        first_run,
    })
}

#[cfg(any(windows, target_os = "linux"))]
fn open_existing(
    path: &Path,
    data_dir: &Path,
    #[cfg(target_os = "linux")] instance: &LinuxInstanceLock,
) -> Result<AppDb, io::Error> {
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
        migrate(
            path,
            &data_dir.join("db-backups"),
            version,
            #[cfg(target_os = "linux")]
            instance,
        )?;
    }
    AppDb::open_gui_with_history(path).map_err(open_error)
}

#[cfg(any(windows, target_os = "linux"))]
fn migrate(
    path: &Path,
    backups: &Path,
    version: u32,
    #[cfg(target_os = "linux")] instance: &LinuxInstanceLock,
) -> Result<(), io::Error> {
    create_backup_directory(backups)
        .map_err(|error| io::Error::other(format!("db.backup_failed: {error}")))?;
    let _directory = prepare_backup_directory(
        backups,
        #[cfg(target_os = "linux")]
        instance,
    )?;
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

#[cfg(windows)]
fn prepare_backup_directory(
    backups: &Path,
) -> Result<du_platform_os::windows_observation::WindowsDirectory, io::Error> {
    open_verified_stable(backups)
        .map_err(|error| io::Error::other(format!("db.backup_failed: {error:?}")))
}

#[cfg(target_os = "linux")]
fn prepare_backup_directory(
    _backups: &Path,
    instance: &LinuxInstanceLock,
) -> Result<fs::File, io::Error> {
    // Pins the no-follow directory object through publication. The cooperative
    // advisory lock does not exclude unrelated same-user filesystem mutation.
    instance
        .open_child_directory("db-backups")
        .map_err(|error| io::Error::other(format!("db.backup_failed: {error:?}")))
}

#[cfg(windows)]
fn create_data_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

#[cfg(target_os = "linux")]
fn create_data_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700).create(path)
}

#[cfg(target_os = "macos")]
fn create_data_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700).create(path)
}

#[cfg(windows)]
fn create_backup_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

#[cfg(target_os = "linux")]
fn create_backup_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder
        .recursive(false)
        .mode(0o700)
        .create(path)
        .or_else(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists
                && fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
            {
                Ok(())
            } else {
                Err(error)
            }
        })
}

#[cfg(any(windows, target_os = "linux"))]
fn report_retention(retention: &RetentionDiagnostic) {
    if retention.inspection_failed || (retention.cleanup_unavailable && retention.content_valid > 3)
    {
        eprintln!("db.backup_retention_unavailable");
    }
}

#[cfg(windows)]
fn lock_error(error: InstanceLockError) -> io::Error {
    match error {
        InstanceLockError::Busy => io::Error::new(
            io::ErrorKind::AlreadyExists,
            "DiskUnfold is already running for this data directory",
        ),
        error => io::Error::other(format!("db.open_failed: {error:?}")),
    }
}

#[cfg(target_os = "linux")]
fn linux_lock_error(error: LinuxInstanceLockError) -> io::Error {
    match error {
        LinuxInstanceLockError::Busy => io::Error::new(
            io::ErrorKind::AlreadyExists,
            "DiskUnfold is already running for this data directory",
        ),
        error => io::Error::other(format!("db.open_failed: {error:?}")),
    }
}

#[cfg(target_os = "macos")]
fn macos_lock_error(error: MacOsInstanceLockError) -> io::Error {
    match error {
        MacOsInstanceLockError::Busy => io::Error::new(
            io::ErrorKind::AlreadyExists,
            "DiskUnfold is already running for this data directory",
        ),
        error => io::Error::other(format!("db.open_failed: {error:?}")),
    }
}

#[cfg(target_os = "macos")]
fn verify_macos_paths(
    instance: &MacOsInstanceLock,
    data_dir: &Path,
    database_file: Option<&fs::File>,
) -> Result<(), io::Error> {
    instance
        .verify_data_directory_path(data_dir)
        .and_then(|()| instance.verify_sentinel_path())
        .and_then(|()| match database_file {
            Some(file) => instance.verify_database_path(file),
            None => Ok(()),
        })
        .map_err(macos_lock_error)
}

fn open_error(error: OpenError) -> io::Error {
    if error.is_corrupt_database() {
        return io::Error::other(format!("db.corrupt: {error:?}"));
    }
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
        _ => "startup.migration_failed",
    };
    io::Error::other(format!("{key}: {error:?}"))
}

#[cfg(all(test, windows))]
mod tests;

#[cfg(all(test, target_os = "linux"))]
mod linux_tests;

#[cfg(all(test, target_os = "macos"))]
mod macos_tests;
