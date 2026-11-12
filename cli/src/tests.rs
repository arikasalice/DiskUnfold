use super::*;

#[derive(Clone, Copy, Debug)]
enum StoredRead {
    Failed,
    Missing,
    UnsupportedLanguage,
}

struct FailingWriter {
    bytes: Vec<u8>,
    fail_after: usize,
}

impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.fail_after.saturating_sub(self.bytes.len());
        if remaining == 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        }
        let written = remaining.min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn help_and_version_do_not_read_stored_settings() {
    for arguments in [vec!["--help"], vec!["--version"]] {
        let mut stored_reads = 0;
        let exit = run_with(
            arguments.into_iter().map(OsString::from),
            || {
                stored_reads += 1;
                Err(())
            },
            || None,
        );
        assert_eq!(exit, ExitCode::SUCCESS);
        assert_eq!(stored_reads, 0);
    }
}

#[test]
fn invalid_help_arguments_keep_the_normal_settings_path() {
    let mut stored_reads = 0;
    let exit = run_with(
        ["--help", "unexpected"].into_iter().map(OsString::from),
        || {
            stored_reads += 1;
            Err(())
        },
        || None,
    );
    assert_eq!(exit, ExitCode::from(2));
    assert_eq!(stored_reads, 1);
}

#[test]
fn not_provided_commands_skip_settings_and_keep_localized_diagnostic() {
    for input in [
        vec!["unregistered-command"],
        vec!["delete"],
        vec!["status"],
        vec!["query"],
    ] {
        let mut stored_reads = 0;
        let mut os_reads = 0;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_io(
            input.iter().map(OsString::from),
            || {
                stored_reads += 1;
                Ok(Some("en".to_owned()))
            },
            || {
                os_reads += 1;
                Some(vec!["ko-KR".to_owned()])
            },
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(exit, ExitCode::from(2), "{input:?}");
        assert_eq!(stored_reads, 0, "{input:?}");
        assert_eq!(os_reads, 1, "{input:?}");
        assert!(stdout.is_empty(), "{input:?}");
        assert_eq!(
            stderr,
            "DiskUnfold: 지원하지 않는 인자입니다. --help를 확인하세요.\n".as_bytes(),
            "{input:?}"
        );
    }
}

#[test]
fn malformed_options_keep_the_normal_settings_path() {
    for input in [
        vec!["--bogus"],
        vec!["-x"],
        vec!["scan", "root", "--execute"],
        vec!["--help", "--execute"],
    ] {
        let mut stored_reads = 0;
        let mut os_reads = 0;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_io(
            input.iter().map(OsString::from),
            || {
                stored_reads += 1;
                Err(())
            },
            || {
                os_reads += 1;
                Some(vec!["ko-KR".to_owned()])
            },
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(exit, ExitCode::from(2), "{input:?}");
        assert_eq!(stored_reads, 1, "{input:?}");
        assert_eq!(os_reads, 1, "{input:?}");
        assert!(stdout.is_empty(), "{input:?}");
        assert_eq!(
            stderr,
            concat!(
                "저장된 설정을 사용할 수 없어 대체 언어를 사용합니다.\n",
                "DiskUnfold: 지원하지 않는 인자입니다. --help를 확인하세요.\n"
            )
            .as_bytes(),
            "{input:?}"
        );
    }
}

#[test]
fn explicit_language_wins_for_not_provided_command_without_settings_access() {
    let mut stored_reads = 0;
    let mut os_reads = 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_with_io(
        ["--lang", "en", "delete"].into_iter().map(OsString::from),
        || {
            stored_reads += 1;
            Err(())
        },
        || {
            os_reads += 1;
            Some(vec!["ko-KR".to_owned()])
        },
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit, ExitCode::from(2));
    assert_eq!(stored_reads, 0);
    assert_eq!(os_reads, 0);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"DiskUnfold: unsupported arguments. See --help.\n");
}

#[test]
fn unregistered_language_keeps_the_existing_usage_error() {
    let mut stored_reads = 0;
    let mut os_reads = 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_with_io(
        ["--lang", "fr", "delete"].into_iter().map(OsString::from),
        || {
            stored_reads += 1;
            Err(())
        },
        || {
            os_reads += 1;
            None
        },
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit, ExitCode::from(2));
    assert_eq!(stored_reads, 0);
    assert_eq!(os_reads, 0);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"DiskUnfold: locale.unsupported\n");
}

#[cfg(windows)]
#[test]
fn not_provided_command_does_not_open_an_isolated_corrupt_settings_database() -> Result<(), String>
{
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use du_core_contracts::settings::SettingsStore;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    struct TempDirectory(PathBuf);

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    let fixture_path = std::env::temp_dir().join(format!(
        "diskunfold-cli-fr6-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&fixture_path).map_err(|error| error.to_string())?;
    let _fixture = TempDirectory(fixture_path.clone());
    let database_path = fixture_path.join("settings.sqlite3");
    fs::write(&database_path, b"intentionally corrupt settings database")
        .map_err(|error| error.to_string())?;

    let database_is_corrupt = match du_app_db::AppDb::open_read_only(&database_path) {
        Err(_) => true,
        Ok(mut database) => database.load().is_err(),
    };
    assert!(database_is_corrupt, "fixture must fail to load as app-db");

    let mut stored_reads = 0;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_with_io(
        ["delete"].into_iter().map(OsString::from),
        || {
            stored_reads += 1;
            let mut database = du_app_db::AppDb::open_read_only(&database_path).map_err(|_| ())?;
            database.load().map_err(|_| ())?;
            Ok(None)
        },
        || Some(vec!["en-US".to_owned()]),
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit, ExitCode::from(2));
    assert_eq!(stored_reads, 0);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"DiskUnfold: unsupported arguments. See --help.\n");
    Ok(())
}

#[test]
fn settings_warning_is_localized_for_invalid_saved_locale_or_read_failure() -> Result<(), String> {
    let registry = embedded::register(&[]).map_err(|_| "embedded bundles register")?;

    for (language, expected_warning, unsupported) in [
        (
            "en",
            "Saved settings are unavailable. Using a fallback language.",
            "DiskUnfold: unsupported arguments. See --help.",
        ),
        (
            "ko",
            "저장된 설정을 사용할 수 없어 대체 언어를 사용합니다.",
            "DiskUnfold: 지원하지 않는 인자입니다. --help를 확인하세요.",
        ),
    ] {
        let bundle = registry
            .bundle(language)
            .ok_or("test language is not registered")?;
        let registered_warning = bundle
            .messages
            .iter()
            .find_map(|(key, value)| {
                (key == SETTINGS_UNAVAILABLE_MESSAGE_KEY).then_some(value.text.as_str())
            })
            .ok_or("settings warning text is not registered")?;
        assert_eq!(registered_warning, expected_warning, "{language}");

        for stored_read in [
            StoredRead::Failed,
            StoredRead::UnsupportedLanguage,
            StoredRead::Missing,
        ] {
            let os_language = format!("{language}-US");
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let exit = run_with_io(
                ["scan", "root"].into_iter().map(OsString::from),
                || match stored_read {
                    StoredRead::Failed => Err(()),
                    StoredRead::Missing => Ok(None),
                    StoredRead::UnsupportedLanguage => Ok(Some("fr".to_owned())),
                },
                || Some(vec![os_language]),
                &mut stdout,
                &mut stderr,
            );

            assert_eq!(exit, ExitCode::from(2), "{language} {stored_read:?}");
            assert!(stdout.is_empty(), "{language} {stored_read:?}");
            let expected_stderr = match stored_read {
                StoredRead::Missing => format!("{unsupported}\n"),
                StoredRead::Failed | StoredRead::UnsupportedLanguage => {
                    format!("{expected_warning}\n{unsupported}\n")
                }
            };
            assert_eq!(
                stderr,
                expected_stderr.as_bytes(),
                "{language} {stored_read:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn missing_settings_warning_translation_falls_back_to_stable_reason_key() {
    let mut stderr = Vec::new();
    write_settings_unavailable_warning(&mut stderr, None);
    assert_eq!(stderr, b"settings_unavailable\n");
}

#[test]
fn stdout_failure_returns_output_interrupted_without_appending() {
    let mut stdout = FailingWriter {
        bytes: Vec::new(),
        fail_after: 3,
    };
    let mut stderr = FailingWriter {
        bytes: Vec::new(),
        fail_after: 0,
    };
    let exit = run_with_io(
        ["--help"].into_iter().map(OsString::from),
        || Err(()),
        || None,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(exit, ExitCode::from(5));
    assert_eq!(stdout.bytes, b"Dis");
    assert!(stderr.bytes.is_empty());
}

#[test]
fn localized_output_interruption_diagnostic_is_one_line() {
    for (message, expected) in [
        (
            "Output was interrupted.",
            b"Output was interrupted.\n".as_slice(),
        ),
        ("출력이 중단됐습니다.", "출력이 중단됐습니다.\n".as_bytes()),
    ] {
        let mut stderr = Vec::new();
        write_output_interrupted_diagnostic(&mut stderr, Some(message));
        assert_eq!(stderr, expected);
    }
}

#[test]
fn closed_stderr_does_not_panic_while_writing_diagnostic() {
    let mut stderr = FailingWriter {
        bytes: Vec::new(),
        fail_after: 0,
    };
    write_output_interrupted_diagnostic(&mut stderr, Some("Output was interrupted."));
    assert!(stderr.bytes.is_empty());
}

#[test]
fn missing_output_interruption_translation_writes_no_placeholder() {
    let mut stderr = Vec::new();
    write_output_interrupted_diagnostic(&mut stderr, None);
    assert!(stderr.is_empty());
}
