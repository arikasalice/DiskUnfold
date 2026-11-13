//! The CLI reads stored settings without creating, repairing or migrating the database.

#[cfg(any(windows, target_os = "linux"))]
use crate::startup_locale::stored_language_in_directory;

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn old_schema_uses_unavailable_settings_without_migration() {
    let snapshot = du_core_contracts::settings::SettingsSnapshot {
        schema_version: 1,
        ..Default::default()
    };
    assert_eq!(
        crate::startup_locale::language_from_snapshot(&snapshot),
        Err(())
    );
}

/// Names, lengths, a byte checksum and modification times of every entry in a directory.
#[cfg(any(windows, target_os = "linux"))]
fn directory_state(
    directory: &std::path::Path,
) -> std::io::Result<Vec<(std::ffi::OsString, usize, u64, std::time::SystemTime)>> {
    let mut state = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let bytes = std::fs::read(entry.path())?;
        let checksum = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        });
        state.push((
            entry.file_name(),
            bytes.len(),
            checksum,
            entry.metadata()?.modified()?,
        ));
    }
    state.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(state)
}

/// Creates a current-format database in `directory`, returning its path and bytes.
#[cfg(any(windows, target_os = "linux"))]
fn current_database(
    directory: &std::path::Path,
) -> Result<(std::path::PathBuf, Vec<u8>), Box<dyn std::error::Error>> {
    let path = directory.join(du_platform_os::application_data::DATABASE_FILE);
    drop(du_app_db::AppDb::create_gui(&path).map_err(|error| format!("{error:?}"))?);
    let bytes = std::fs::read(&path)?;
    Ok((path, bytes))
}

#[cfg(any(windows, target_os = "linux"))]
fn fixture_directory() -> Result<std::path::PathBuf, std::io::Error> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);
    let directory = std::env::temp_dir().join(format!(
        "diskunfold-cli-readonly-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory)?;
    Ok(directory)
}

/// AGENTS.md: the CLI never creates, repairs or migrates the database.
#[cfg(any(windows, target_os = "linux"))]
#[test]
fn reading_stored_language_leaves_missing_older_and_newer_rollback_journal_databases_untouched()
-> Result<(), Box<dyn std::error::Error>> {
    use std::fs;

    let directory = fixture_directory()?;
    let database_path = directory.join(du_platform_os::application_data::DATABASE_FILE);

    // Missing database: nothing may be created.
    assert_eq!(stored_language_in_directory(&directory), Ok(None));
    assert!(directory_state(&directory)?.is_empty());

    let (_, current) = current_database(&directory)?;
    let _ = fs::remove_file(&database_path);

    // SQLite keeps user_version at byte offset 60 and the journal mode at
    // bytes 18 and 19 (1 = rollback journal, 2 = WAL) of the header.
    for (label, user_version) in [("older", 0_u32), ("newer", 2_u32), ("current", 1_u32)] {
        let mut bytes = current.clone();
        bytes[60..64].copy_from_slice(&user_version.to_be_bytes());
        bytes[18] = 1;
        bytes[19] = 1;
        fs::write(&database_path, &bytes)?;
        let before = directory_state(&directory)?;
        let _ = stored_language_in_directory(&directory);
        assert_eq!(directory_state(&directory)?, before, "{label}");
    }
    let _ = fs::remove_dir_all(&directory);
    Ok(())
}

/// A WAL database that its writer closed cleanly has no `-wal` or `-shm` file;
/// reading the stored language must not create them.
#[cfg(any(windows, target_os = "linux"))]
#[test]
fn reading_stored_language_leaves_a_wal_database_untouched()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = fixture_directory()?;
    current_database(&directory)?;
    let before = directory_state(&directory)?;
    let _ = stored_language_in_directory(&directory);
    assert_eq!(directory_state(&directory)?, before);
    let _ = std::fs::remove_dir_all(&directory);
    Ok(())
}
