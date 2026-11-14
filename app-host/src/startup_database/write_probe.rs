//! Proves that the data directory accepts new files when an existing database is reopened.
//!
//! A first start proves it by publishing the database. A reopen only opens files that already
//! exist, so this module creates and removes a small file in the directory instead. The caller
//! holds the instance lock, so no other DiskUnfold process uses the same name.
use std::{fs, io, path::Path};

use super::data_folder_error;

const PROBE_NAME: &str = ".diskunfold.write-probe";

/// Creates and removes the probe file in `data_dir`.
///
/// # Errors
///
/// `startup.data_dir_unwritable` when the directory refuses a new file or the probe cannot be
/// removed.
pub(super) fn verify(data_dir: &Path) -> Result<(), io::Error> {
    let path = data_dir.join(PROBE_NAME);
    // A crash can leave the probe behind; it holds no data.
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(data_folder_error(Some(error))),
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| data_folder_error(Some(error)))?;
    fs::remove_file(&path).map_err(|error| data_folder_error(Some(error)))
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Regression for the relaunch defect: a second start on an existing data directory showed
    /// "could not verify" for the local data row, while a fresh directory showed a pass.
    #[test]
    fn reopening_an_existing_data_directory_passes_the_startup_data_row() -> TestResult {
        use crate::startup_checks::{data_format_and_open_observation, status};
        use du_core_contracts::startup_ipc::{
            StartupAccessState, StartupCheck, StartupCheckOutcome,
        };
        let dir = tempfile::tempdir()?;
        let first = super::super::open_with_status(dir.path())?;
        assert!(first.first_run);
        drop(first.database);
        for _ in 0..2 {
            let reopened = super::super::open_with_status(dir.path())?;
            assert!(!reopened.first_run);
            let observed = [data_format_and_open_observation()];
            let report = status(false, StartupAccessState::Confirmed, &observed)
                .map_err(|error| format!("{error:?}"))?;
            let row = report
                .checks
                .iter()
                .find(|row| row.check == StartupCheck::DataFormatAndOpen)
                .ok_or("data row")?;
            assert_eq!(row.result, StartupCheckOutcome::Pass);
            drop(reopened.database);
        }
        assert!(!dir.path().join(PROBE_NAME).exists());
        Ok(())
    }

    #[test]
    fn a_writable_directory_passes_and_keeps_no_probe_file() -> TestResult {
        let dir = tempfile::tempdir()?;
        verify(dir.path())?;
        assert_eq!(fs::read_dir(dir.path())?.count(), 0);
        // A probe left by a crash does not fail the next start.
        fs::write(dir.path().join(PROBE_NAME), b"")?;
        verify(dir.path())?;
        assert_eq!(fs::read_dir(dir.path())?.count(), 0);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_refuses_new_files_reports_the_data_folder_reason() -> TestResult {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500))?;
        let refused = fs::write(dir.path().join("probe-check"), b"").is_err();
        let result = verify(dir.path());
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
        if refused {
            let error = result.err().ok_or("expected a refusal")?;
            assert!(error.to_string().starts_with("startup.data_dir_unwritable"));
        }
        Ok(())
    }
}
