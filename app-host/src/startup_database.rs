//! Open the GUI database under a platform instance lock before restoring settings.

use std::{fs, io, path::Path};

use du_app_db::{
    AppDb, HISTORY_DEFINITIONS, HISTORY_SCHEMA_VERSION, OpenError,
    migration::{
        MigrationError,
        managed::{
            JOURNAL, ManagedRunFailure, RecoveryOutcome, RestoreStatus, RetentionDiagnostic,
            apply_managed_run, recover_interrupted,
        },
    },
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

#[cfg(target_os = "linux")]
type Instance = LinuxInstanceLock;
#[cfg(target_os = "macos")]
type Instance = MacOsInstanceLock;

// F14 DLD rev79 §2.5.5 keeps the steps a failed migration already applied. The user decided to
// follow F15 HLD rev116 §2.3.3.5 and DLD rev70 §2.6 (FR8.HD1.app-db.DD1, FR8.HD1.app-host.DD1)
// instead: a failed migration restores the whole pre-migration backup on every platform. F14
// DLD79 §2.5.5 needs a design revision (docs/handoff/F14-DLD79-2.5.5-needs-revision.md).
// Both outcomes use the published HLD reason startup.migration_failed; the design has no
// separate key for a failed restore, so a stable sub-reason after the key (restored or
// restore_failed) says what happened. The sub-reasons need design confirmation (see the handoff
// note). Internal error detail is logged to stderr and never placed in the reason.

/// Logs internal detail for diagnosis; the user-facing reason carries only a stable key.
fn log_detail(reason: &str, detail: &dyn std::fmt::Debug) {
    eprintln!("{reason}: {detail:?}");
}

/// Builds a reason that contains only a stable key and logs the detail separately.
fn reason(key: &str, detail: &dyn std::fmt::Debug) -> io::Error {
    log_detail(key, detail);
    io::Error::other(key.to_owned())
}

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
    finish_interrupted_migration(&path, &data_dir.join("db-backups"))?;
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
/// Lower-format databases follow the same backup, migrate and full-restore path as Windows and
/// Linux. Restoring writes into the pinned database file instead of replacing it, so the path
/// identity checks stay valid, and they run again around every phase. The SQLite read-only probe
/// may initialize WAL sidecars; it preserves the main database and existing WAL frames and
/// leaves sidecars under SQLite's normal locking and recovery behavior.
#[cfg(target_os = "macos")]
pub(super) fn open_with_status(data_dir: &Path) -> Result<OpenedDatabase, io::Error> {
    let data_dir = std::path::absolute(data_dir).map_err(|error| data_folder_error(Some(error)))?;
    create_data_directory(&data_dir).map_err(|error| data_folder_error(Some(error)))?;
    let instance = MacOsInstanceLock::acquire(&data_dir).map_err(macos_lock_error)?;
    verify_macos_paths(&instance, &data_dir, None)?;
    let path = data_dir.join(DATABASE_FILE);
    finish_interrupted_migration(&path, &data_dir.join("db-backups"), &instance)?;
    verify_macos_paths(&instance, &data_dir, None)?;
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
        let result = migrate(path, &data_dir.join("db-backups"), version, instance);
        verify_macos_paths(instance, data_dir, Some(pinned_file))?;
        result?;
    }
    if version > HISTORY_SCHEMA_VERSION {
        return Err(io::Error::other("db.schema_mismatch"));
    }
    let database = if version < HISTORY_SCHEMA_VERSION {
        AppDb::open_gui_with_history(path).map_err(open_error)?
    } else {
        AppDb::open_gui_with_history_after_version_probe(path).map_err(open_error)?
    };
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
    finish_interrupted_migration(&path, &data_dir.join("db-backups"), &instance)?;
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

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn migrate(
    path: &Path,
    backups: &Path,
    version: u32,
    #[cfg(unix)] instance: &Instance,
) -> Result<(), io::Error> {
    create_backup_directory(backups).map_err(|error| reason("db.backup_failed", &error))?;
    let _directory = prepare_backup_directory(
        backups,
        #[cfg(unix)]
        instance,
    )?;
    let outcome = match apply_managed_run(path, backups, &HISTORY_DEFINITIONS, version) {
        Ok(outcome) => outcome,
        Err(failure) => {
            report_retention(&failure.retention);
            return Err(run_failure(*failure));
        }
    };
    report_retention(&outcome.retention);
    if outcome.revision != HISTORY_SCHEMA_VERSION {
        return Err(io::Error::other("db.schema_mismatch"));
    }
    Ok(())
}

/// Finishes a migration or restore that a previous run left unfinished, before anything else
/// reads the database. A journal that is found always blocks this start: the restore result is
/// reported and the next start migrates again from the restored database.
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn finish_interrupted_migration(
    path: &Path,
    backups: &Path,
    #[cfg(unix)] instance: &Instance,
) -> Result<(), io::Error> {
    // A database whose marker-route creation was killed is removed so that it is created again
    // (FR3.HD1.app-db.DD1); the instance lock is held, so no other creator runs.
    AppDb::recover_incomplete_creation(path).map_err(|error| reason("db.open_failed", &error))?;
    // Only a leftover journal needs recovery; an ordinary start does not touch the backups.
    if !journal_pending(fs::symlink_metadata(backups.join(JOURNAL)))? {
        return Ok(());
    }
    let _directory = prepare_backup_directory(
        backups,
        #[cfg(unix)]
        instance,
    )?;
    match recover_interrupted(path, backups, &HISTORY_DEFINITIONS) {
        Ok(RecoveryOutcome::NothingPending | RecoveryOutcome::AlreadyComplete { .. }) => Ok(()),
        Ok(RecoveryOutcome::Restored(restored)) => {
            log_detail(
                "startup.migration_failed: interrupted migration restored",
                &restored,
            );
            Err(io::Error::other(RESTORED))
        }
        Err(error) => Err(restore_failed(&error, None)),
    }
}

const RESTORED: &str = "startup.migration_failed: restored";
const RESTORE_FAILED: &str = "startup.migration_failed: restore_failed";
const JOURNAL_UNREADABLE: &str = "startup.migration_failed: journal_unreadable";

/// Decides from the journal probe whether recovery is needed. A missing journal means no, and so
/// does a backup path that is not a directory (nothing can be stored below it; the migration
/// then reports the backup obstacle). Every other error fails closed because an unreadable
/// journal may hide an interrupted run.
fn journal_pending(probe: io::Result<fs::Metadata>) -> io::Result<bool> {
    match probe {
        Ok(_) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        }
        Err(error) => {
            log_detail(JOURNAL_UNREADABLE, &error);
            Err(io::Error::other(JOURNAL_UNREADABLE))
        }
    }
}

/// Maps a failed run to the startup reason, naming the restore result when a restore ran.
fn run_failure(failure: ManagedRunFailure) -> io::Error {
    match failure.restore {
        RestoreStatus::NotNeeded => migration_error(failure.migration),
        RestoreStatus::Restored(restored) => {
            log_detail(RESTORED, &(&failure.migration, &restored));
            io::Error::other(RESTORED)
        }
        RestoreStatus::Failed(error) => restore_failed(&error, Some(&failure.migration)),
    }
}

/// Fail-closed report: the database, the backups and the journal are all kept.
fn restore_failed(error: &MigrationError, cause: Option<&MigrationError>) -> io::Error {
    log_detail(RESTORE_FAILED, &(error, cause));
    io::Error::other(RESTORE_FAILED)
}

#[cfg(windows)]
fn prepare_backup_directory(
    backups: &Path,
) -> Result<du_platform_os::windows_observation::WindowsDirectory, io::Error> {
    open_verified_stable(backups).map_err(|error| reason("db.backup_failed", &error))
}

#[cfg(unix)]
fn prepare_backup_directory(_backups: &Path, instance: &Instance) -> Result<fs::File, io::Error> {
    // Pins the no-follow directory object through publication. The cooperative
    // advisory lock does not exclude unrelated same-user filesystem mutation.
    instance
        .open_child_directory("db-backups")
        .map_err(|error| reason("db.backup_failed", &error))
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

#[cfg(unix)]
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

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
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
        error => reason("db.open_failed", &error),
    }
}

#[cfg(target_os = "linux")]
fn linux_lock_error(error: LinuxInstanceLockError) -> io::Error {
    match error {
        LinuxInstanceLockError::Busy => io::Error::new(
            io::ErrorKind::AlreadyExists,
            "DiskUnfold is already running for this data directory",
        ),
        error => reason("db.open_failed", &error),
    }
}

#[cfg(target_os = "macos")]
fn macos_lock_error(error: MacOsInstanceLockError) -> io::Error {
    match error {
        MacOsInstanceLockError::Busy => io::Error::new(
            io::ErrorKind::AlreadyExists,
            "DiskUnfold is already running for this data directory",
        ),
        error => reason("db.open_failed", &error),
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
        return reason("db.corrupt", &error);
    }
    match error {
        OpenError::UnsupportedSchema(_) => io::Error::other("db.schema_mismatch"),
        OpenError::HistorySchema(error) => migration_error(*error),
        error => reason("db.open_failed", &error),
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
    reason(key, &error)
}

#[cfg(all(test, windows))]
mod tests;

#[cfg(all(test, any(windows, target_os = "linux", target_os = "macos")))]
mod restore_tests;

#[cfg(all(test, any(windows, target_os = "linux", target_os = "macos")))]
mod upgrade_tests;

#[cfg(all(test, target_os = "linux"))]
mod linux_tests;

#[cfg(all(test, target_os = "macos"))]
mod macos_tests;
