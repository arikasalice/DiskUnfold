//! Startup row 4: product temporary files left in the data folder.
use std::{io, path::Path};

use du_core_contracts::startup_ipc::{StartupCheck, StartupCheckOutcome, StartupCheckResult};

/// Reason key published for a temporary file that an interrupted operation left.
pub(crate) const TEMP_FILE_LEFT: &str = "export.temp_file_left";

/// Name of the managed backup folder inside the data folder (app-db migration).
const BACKUP_DIRECTORY: &str = "db-backups";

/// Temporary name of the migration journal that `app-db` writes before publishing it.
const JOURNAL_TEMPORARY: &str = "migration-journal.json.tmp";

/// Names the storage adapters give files they create before publishing them.
///
/// - `.diskunfold-create-<n>-<id>.tmp`: `app-db` first database creation (FR3.HD1.app-db.DD1).
/// - `.tmp-<id>.dusnap.part`: the snapshot staging file of `platform-os`.
fn is_data_folder_temporary_name(name: &str) -> bool {
    (name.starts_with(".diskunfold-") && name.ends_with(".tmp"))
        || (name.starts_with(".tmp-") && name.ends_with(".dusnap.part"))
}

fn row(result: StartupCheckOutcome, reason_key: Option<&str>) -> StartupCheckResult {
    StartupCheckResult {
        check: StartupCheck::TemporaryFiles,
        result,
        reason_key: reason_key.map(str::to_owned),
    }
}

/// What listing one folder found.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Listing {
    /// A leftover product temporary file is present.
    Leftover,
    /// The whole folder was listed and holds no leftover.
    Clean,
    /// The folder, or some entry in it, could not be read.
    Incomplete,
}

/// Lists `dir` once and reports whether `is_temporary` matches any entry name.
fn list(dir: &Path, is_temporary: impl Fn(&str) -> bool) -> Listing {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Listing::Incomplete;
    };
    let mut complete = true;
    for entry in entries {
        match entry {
            Ok(entry) => {
                if entry.file_name().to_str().is_some_and(&is_temporary) {
                    return Listing::Leftover;
                }
            }
            Err(_) => complete = false,
        }
    }
    if complete {
        Listing::Clean
    } else {
        Listing::Incomplete
    }
}

/// Lists the managed backup folder. A folder that does not exist is clean, and a link
/// is never followed because the startup database path rejects it as well.
fn list_backup_directory(data_dir: &Path) -> Listing {
    let backups = data_dir.join(BACKUP_DIRECTORY);
    match std::fs::symlink_metadata(&backups) {
        Ok(meta) if meta.is_dir() => list(&backups, |name| name == JOURNAL_TEMPORARY),
        Ok(_) => Listing::Incomplete,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Listing::Clean,
        Err(_) => Listing::Incomplete,
    }
}

/// Inspects the data folder for leftover product temporary files without changing it.
///
/// The data folder and its managed backup folder are listed once each. A leftover
/// file reports `Limited` with `export.temp_file_left`. A folder that cannot be
/// listed completely stays `Unverifiable` unless a leftover was already seen. The
/// check never deletes: another process may be mid-publication.
pub(crate) fn temporary_files_observation(data_dir: &Path) -> StartupCheckResult {
    let data = list(data_dir, is_data_folder_temporary_name);
    if data == Listing::Leftover {
        return row(StartupCheckOutcome::Limited, Some(TEMP_FILE_LEFT));
    }
    let backups = if data == Listing::Incomplete {
        Listing::Incomplete
    } else {
        list_backup_directory(data_dir)
    };
    match backups {
        Listing::Leftover => row(StartupCheckOutcome::Limited, Some(TEMP_FILE_LEFT)),
        Listing::Clean => row(StartupCheckOutcome::Pass, None),
        Listing::Incomplete => row(StartupCheckOutcome::Unverifiable, None),
    }
}

/// Observes the host's resolved data folder; an unresolved folder is unverifiable.
pub(crate) fn host_temporary_files_observation() -> StartupCheckResult {
    du_platform_os::application_data::directory().map_or_else(
        || row(StartupCheckOutcome::Unverifiable, None),
        |dir| temporary_files_observation(&dir),
    )
}
