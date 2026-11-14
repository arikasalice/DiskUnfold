//! Stored display-language reads through the CLI's read-only settings path.

use std::{
    error::Error,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicU64, Ordering},
};

use du_core_contracts::settings::{SettingsChanges, SettingsCommit, SettingsSource, SettingsStore};

use super::*;

const WARNING: &str = "Saved settings are unavailable. Using a fallback language.";

/// A unique temporary data directory that is removed on drop.
struct Fixture(PathBuf);

impl Fixture {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "diskunfold-cli-f13-stored-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn database_path(directory: &Path) -> PathBuf {
    directory.join(du_platform_os::application_data::DATABASE_FILE)
}

/// Creates a current-format database whose display language is `language`.
fn write_language(directory: &Path, language: &str) -> Result<(), Box<dyn Error>> {
    let mut database = du_app_db::AppDb::create_gui(&database_path(directory))
        .map_err(|error| format!("{error:?}"))?;
    let changes = SettingsChanges::from([(
        locale::DISPLAY_LANGUAGE_KEY.into(),
        Some(serde_json::Value::String(language.to_owned())),
    )]);
    database
        .commit(
            0,
            &changes,
            SettingsCommit {
                source: SettingsSource::Screen,
                timestamp_ms: 1,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

/// Every entry of `directory` with its bytes, in name order.
fn directory_bytes(directory: &Path) -> io::Result<Vec<(OsString, Vec<u8>)>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        entries.push((entry.file_name(), fs::read(entry.path())?));
    }
    entries.sort();
    Ok(entries)
}

/// Selects a language for a command that reads stored settings, with an injected OS list.
fn select(
    directory: &Path,
    os_tag: &str,
    os_reads: &mut u32,
) -> Result<(Decision, bool), Box<dyn Error>> {
    let registry = i18n::embedded::register(&[]).map_err(|error| format!("{error:?}"))?;
    select_with_policy(
        &registry,
        None,
        false,
        || stored_language_in_directory(directory),
        || {
            *os_reads += 1;
            Some(vec![os_tag.to_owned()])
        },
    )
    .map_err(|error| format!("{error:?}").into())
}

/// Runs `scan root` (a command that reads stored settings) and returns exit code, OS reads and stderr.
fn run_scan(directory: &Path, os_tag: &str) -> (ExitCode, u32, String) {
    let mut os_reads = 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = crate::run_with_io(
        ["scan", "root"].into_iter().map(OsString::from),
        || stored_language_in_directory(directory),
        || {
            os_reads += 1;
            Some(vec![os_tag.to_owned()])
        },
        &mut stdout,
        &mut stderr,
    );
    (
        exit,
        os_reads,
        String::from_utf8_lossy(&stderr).into_owned(),
    )
}

/// A stored language outranks the OS list in either direction and the read changes nothing.
#[test]
fn stored_language_wins_over_the_operating_system_list() -> Result<(), Box<dyn Error>> {
    for (stored, os_tag) in [("ko", "en-US"), ("en", "ko-KR")] {
        let fixture = Fixture::new()?;
        write_language(&fixture.0, stored)?;
        let before = directory_bytes(&fixture.0)?;

        let mut os_reads = 0;
        let (decision, unavailable) = select(&fixture.0, os_tag, &mut os_reads)?;
        assert_eq!(decision.language, stored);
        assert_eq!(decision.source, locale::Source::Stored);
        assert!(!unavailable);
        assert_eq!(os_reads, 0, "a usable stored language skips the OS lookup");

        let (exit, os_reads, stderr) = run_scan(&fixture.0, os_tag);
        assert_eq!(exit, ExitCode::from(2));
        assert_eq!(os_reads, 0);
        assert!(!stderr.contains(WARNING), "{stderr}");
        assert_eq!(directory_bytes(&fixture.0)?, before, "stored {stored}");
    }
    Ok(())
}

/// No folder or no database means nothing stored: nothing is created and the OS row decides.
#[test]
fn missing_folder_or_database_creates_nothing_and_uses_the_os_row() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let absent = fixture.0.join("absent");
    assert_eq!(stored_language_in_directory(&absent), Ok(None));
    assert!(!absent.exists());
    let mut os_reads = 0;
    let (decision, unavailable) = select(&absent, "ko-KR", &mut os_reads)?;
    assert_eq!(decision.language, "ko");
    assert_eq!(decision.source, locale::Source::Os);
    assert!(!unavailable);
    assert_eq!(os_reads, 1);
    assert!(!absent.exists());

    assert_eq!(stored_language_in_directory(&fixture.0), Ok(None));
    let mut os_reads = 0;
    let (decision, unavailable) = select(&fixture.0, "en-US", &mut os_reads)?;
    assert_eq!(decision.language, "en");
    assert_eq!(decision.source, locale::Source::Os);
    assert!(!unavailable);
    assert!(directory_bytes(&fixture.0)?.is_empty());

    let (exit, os_reads, stderr) = run_scan(&fixture.0, "en-US");
    assert_eq!(exit, ExitCode::from(2));
    assert_eq!(os_reads, 1);
    assert!(!stderr.contains(WARNING), "{stderr}");
    assert!(directory_bytes(&fixture.0)?.is_empty());
    Ok(())
}

/// A database that cannot be read, or names an unregistered language, warns on one line,
/// falls through to the OS row and keeps every byte of the data directory.
#[test]
fn unusable_stored_settings_warn_once_and_leave_the_directory_unchanged()
-> Result<(), Box<dyn Error>> {
    type Prepare = fn(&Path) -> Result<(), Box<dyn Error>>;
    let cases: [(&str, Prepare); 3] = [
        ("corrupt", |directory| {
            fs::write(database_path(directory), vec![0x5a_u8; 4096])?;
            Ok(())
        }),
        ("unregistered", |directory| write_language(directory, "fr")),
        ("wal without shared memory", |directory| {
            write_language(directory, "ko")?;
            let mut wal = database_path(directory).into_os_string();
            wal.push("-wal");
            fs::write(wal, [1_u8; 64])?;
            Ok(())
        }),
    ];
    for (label, prepare) in cases {
        let fixture = Fixture::new()?;
        prepare(&fixture.0)?;
        let before = directory_bytes(&fixture.0)?;
        let (exit, os_reads, stderr) = run_scan(&fixture.0, "en-US");
        assert_eq!(exit, ExitCode::from(2), "{label}");
        assert_eq!(os_reads, 1, "{label}");
        assert_eq!(stderr.matches(WARNING).count(), 1, "{label}: {stderr}");
        assert_eq!(directory_bytes(&fixture.0)?, before, "{label}");
    }
    Ok(())
}
