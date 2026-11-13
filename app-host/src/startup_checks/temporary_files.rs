//! Startup row 4: product temporary files left in the data folder.
use std::path::Path;

use du_core_contracts::startup_ipc::{StartupCheck, StartupCheckOutcome, StartupCheckResult};

/// Reason key published for a temporary file that an interrupted operation left.
pub(crate) const TEMP_FILE_LEFT: &str = "export.temp_file_left";

/// Names the storage adapter gives files it creates before publishing them.
fn is_product_temporary_name(name: &str) -> bool {
    name.starts_with(".diskunfold-") && name.ends_with(".tmp")
}

fn row(result: StartupCheckOutcome, reason_key: Option<&str>) -> StartupCheckResult {
    StartupCheckResult {
        check: StartupCheck::TemporaryFiles,
        result,
        reason_key: reason_key.map(str::to_owned),
    }
}

/// Inspects the data folder for leftover product temporary files without changing it.
///
/// A leftover file reports `Limited` with `export.temp_file_left`. A folder that
/// cannot be listed completely stays `Unverifiable` unless a leftover was already
/// seen. The check never deletes: another process may be mid-publication.
pub(crate) fn temporary_files_observation(data_dir: &Path) -> StartupCheckResult {
    let Ok(entries) = std::fs::read_dir(data_dir) else {
        return row(StartupCheckOutcome::Unverifiable, None);
    };
    let mut complete = true;
    for entry in entries {
        match entry {
            Ok(entry) => {
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(is_product_temporary_name)
                {
                    return row(StartupCheckOutcome::Limited, Some(TEMP_FILE_LEFT));
                }
            }
            Err(_) => complete = false,
        }
    }
    if complete {
        row(StartupCheckOutcome::Pass, None)
    } else {
        row(StartupCheckOutcome::Unverifiable, None)
    }
}

/// Observes the host's resolved data folder; an unresolved folder is unverifiable.
pub(crate) fn host_temporary_files_observation() -> StartupCheckResult {
    du_platform_os::application_data::directory().map_or_else(
        || row(StartupCheckOutcome::Unverifiable, None),
        |dir| temporary_files_observation(&dir),
    )
}
