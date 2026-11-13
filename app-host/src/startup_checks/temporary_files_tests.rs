//! Leftover temporary file detection for startup row 4 (F15 FR4.HD1.app-host.DD1, FR3.HD1.app-db.DD1).
use super::temporary_files::{TEMP_FILE_LEFT, temporary_files_observation};
use du_core_contracts::startup_ipc::StartupCheckOutcome;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn assert_limited(dir: &std::path::Path) {
    let found = temporary_files_observation(dir);
    assert_eq!(found.result, StartupCheckOutcome::Limited);
    assert_eq!(found.reason_key.as_deref(), Some(TEMP_FILE_LEFT));
}

#[test]
fn a_leftover_migration_journal_temp_in_the_backup_folder_is_reported() -> TestResult {
    let dir = tempfile::tempdir()?;
    let backups = dir.path().join("db-backups");
    std::fs::create_dir(&backups)?;
    std::fs::write(backups.join("migration-journal.json"), b"{}")?;
    assert_eq!(
        temporary_files_observation(dir.path()).result,
        StartupCheckOutcome::Pass
    );
    let leftover = backups.join("migration-journal.json.tmp");
    std::fs::write(&leftover, b"half")?;
    assert_limited(dir.path());
    // Observation only: the owner (migration recovery) removes it.
    assert!(leftover.exists());
    assert!(backups.join("migration-journal.json").exists());
    Ok(())
}

#[test]
fn a_leftover_snapshot_staging_file_is_reported() -> TestResult {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("a.dusnap"), b"published")?;
    assert_eq!(
        temporary_files_observation(dir.path()).result,
        StartupCheckOutcome::Pass
    );
    let leftover = dir.path().join(".tmp-abc123.dusnap.part");
    std::fs::write(&leftover, b"partial")?;
    assert_limited(dir.path());
    assert!(leftover.exists());
    Ok(())
}

#[test]
fn similar_names_that_the_product_does_not_create_are_not_reported() -> TestResult {
    let dir = tempfile::tempdir()?;
    let backups = dir.path().join("db-backups");
    std::fs::create_dir(&backups)?;
    for name in [
        "x.dusnap.part",
        ".tmp-x.dusnap",
        "tmp-x.dusnap.part",
        "migration-journal.json.tmp.bak",
        "journal.tmp",
    ] {
        std::fs::write(dir.path().join(name), b"user")?;
    }
    // The journal temp name only matters inside the backup folder.
    std::fs::write(dir.path().join("migration-journal.json.tmp"), b"user")?;
    // Other names in the backup folder are backups, not leftovers.
    std::fs::write(backups.join("backup-1.sqlite.tmp"), b"user")?;
    assert_eq!(
        temporary_files_observation(dir.path()).result,
        StartupCheckOutcome::Pass
    );
    Ok(())
}

#[test]
fn a_backup_path_that_is_not_a_listable_folder_stays_unverifiable() -> TestResult {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("db-backups"), b"not a folder")?;
    assert_eq!(
        temporary_files_observation(dir.path()).result,
        StartupCheckOutcome::Unverifiable
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_linked_backup_folder_is_not_followed() -> TestResult {
    let dir = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    std::fs::write(outside.path().join("migration-journal.json.tmp"), b"x")?;
    std::os::unix::fs::symlink(outside.path(), dir.path().join("db-backups"))?;
    assert_eq!(
        temporary_files_observation(dir.path()).result,
        StartupCheckOutcome::Unverifiable
    );
    Ok(())
}

#[test]
fn a_leftover_in_the_data_folder_wins_over_an_unreadable_backup_path() -> TestResult {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("db-backups"), b"not a folder")?;
    std::fs::write(dir.path().join(".diskunfold-create-1-0.tmp"), b"half")?;
    assert_limited(dir.path());
    Ok(())
}
