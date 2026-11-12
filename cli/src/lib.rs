//! Parse CLI arguments and present engine results.

use std::{
    ffi::OsString,
    io::{self, Write},
    process::ExitCode,
};

use du_user_env::{i18n::embedded, locale::DecisionError};

mod args;
mod startup_locale;

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
        write_stderr_line(stderr, "settings_unavailable");
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
