//! Parse CLI arguments and present engine results.

use std::{
    ffi::OsString,
    io::{self, Write},
    process::ExitCode,
};

use du_user_env::{i18n::embedded, locale::DecisionError};

mod args;
mod startup_locale;

const SETTINGS_UNAVAILABLE_REASON_KEY: &str = "settings_unavailable";
const SETTINGS_UNAVAILABLE_MESSAGE_KEY: &str = "cli.settings.unavailable";

pub fn run(arguments: impl Iterator<Item = OsString>) -> ExitCode {
    #[cfg(all(windows, feature = "windows-console"))]
    du_platform_os::windows_console::attach_parent();

    run_with(
        arguments,
        startup_locale::stored_language,
        startup_locale::preferred_ui_languages,
    )
}

fn run_with(
    arguments: impl Iterator<Item = OsString>,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
) -> ExitCode {
    let stdout = io::stdout();
    let stderr = io::stderr();
    run_with_io(
        arguments,
        read_stored,
        read_os,
        &mut stdout.lock(),
        &mut stderr.lock(),
    )
}

/// Runs a parsed CLI request against explicit streams for error handling tests.
fn run_with_io<W: Write, E: Write>(
    arguments: impl Iterator<Item = OsString>,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
    stdout: &mut W,
    stderr: &mut E,
) -> ExitCode {
    let Some(args) = args::parse(arguments) else {
        write_stderr_line(stderr, "DiskUnfold: locale.unsupported");
        return ExitCode::from(2);
    };
    let Ok(registry) = embedded::register(&[]) else {
        write_stderr_line(stderr, "DiskUnfold: locale.registration_failed");
        return ExitCode::FAILURE;
    };
    let skip_stored_settings =
        matches!(&args.command, args::Command::Help | args::Command::Version);
    let (selection, settings_unavailable) = match startup_locale::select_with_policy(
        &registry,
        args.language.as_deref(),
        skip_stored_settings,
        read_stored,
        read_os,
    ) {
        Ok(selection) => selection,
        Err(DecisionError::UnsupportedLanguage) => {
            write_stderr_line(stderr, "DiskUnfold: locale.unsupported");
            return ExitCode::from(2);
        }
        Err(DecisionError::RegistrationFailed) => {
            write_stderr_line(stderr, "DiskUnfold: locale.registration_failed");
            return ExitCode::FAILURE;
        }
    };
    let Some(bundle) = registry.bundle(&selection.language) else {
        write_stderr_line(stderr, "DiskUnfold: locale.registration_failed");
        return ExitCode::FAILURE;
    };
    let message = |key| {
        bundle
            .messages
            .iter()
            .find_map(|(candidate, message)| (candidate == key).then_some(message.text.as_str()))
    };
    let Some(product_name) = message("ui.product.name") else {
        write_stderr_line(stderr, "DiskUnfold: locale.registration_failed");
        return ExitCode::FAILURE;
    };
    if settings_unavailable || selection.stored_unavailable {
        write_settings_unavailable_warning(stderr, message(SETTINGS_UNAVAILABLE_MESSAGE_KEY));
    }
    for warning in registry.integrity_warnings() {
        let warning = format!(
            "{warning}: {}",
            message(warning.as_str()).unwrap_or(warning)
        );
        write_stderr_line(stderr, &warning);
    }
    let output = match args.command {
        args::Command::Help => {
            let Some(usage) = message("cli.help.usage") else {
                write_stderr_line(stderr, "DiskUnfold: locale.registration_failed");
                return ExitCode::FAILURE;
            };
            write!(stdout, "{product_name}\n\n{usage}\n")
        }
        args::Command::Version => writeln!(stdout, "{product_name} {}", env!("CARGO_PKG_VERSION")),
        // Scan admission is parsed, but execution awaits the engine and policy composition.
        args::Command::Scan(_) | args::Command::Unsupported => {
            let Some(unsupported) = message("cli.help.unsupported") else {
                write_stderr_line(stderr, "DiskUnfold: locale.registration_failed");
                return ExitCode::FAILURE;
            };
            write_stderr_line(stderr, unsupported);
            return ExitCode::from(2);
        }
    };
    if output.is_err() {
        write_output_interrupted_diagnostic(stderr, message("cli.output.interrupted"));
    }
    output_exit_code(output)
}

/// Maps a completed or interrupted stdout write to its process exit code.
fn output_exit_code(write_result: io::Result<()>) -> ExitCode {
    if write_result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(5)
    }
}

/// Writes a diagnostic line without panicking when stderr has been closed.
fn write_stderr_line(stderr: &mut impl Write, message: &str) {
    let _ = writeln!(stderr, "{message}");
}

/// Writes the localized text message, preserving the stable reason key if the
/// corresponding locale resource is missing. Machine-readable stderr records
/// are not emitted by this text-only path; when implemented, they must retain
/// the published stable reason key.
fn write_settings_unavailable_warning(stderr: &mut impl Write, localized_message: Option<&str>) {
    write_stderr_line(
        stderr,
        localized_message.unwrap_or(SETTINGS_UNAVAILABLE_REASON_KEY),
    );
}

/// Writes the registered text diagnostic once when its locale message exists.
fn write_output_interrupted_diagnostic(stderr: &mut impl Write, localized_message: Option<&str>) {
    if let Some(message) = localized_message {
        write_stderr_line(stderr, message);
    }
}

#[cfg(test)]
mod tests {
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
    fn settings_warning_is_localized_for_invalid_saved_locale_or_read_failure() -> Result<(), String>
    {
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
}
