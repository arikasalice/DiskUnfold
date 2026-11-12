//! Parse CLI arguments and present engine results.

use std::{ffi::OsString, process::ExitCode};

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
    let Some(args) = args::parse(arguments) else {
        return unsupported_language();
    };
    let Ok(registry) = embedded::register(&[]) else {
        return registration_failed();
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
        Err(DecisionError::UnsupportedLanguage) => return unsupported_language(),
        Err(DecisionError::RegistrationFailed) => return registration_failed(),
    };
    let Some(bundle) = registry.bundle(&selection.language) else {
        return registration_failed();
    };
    let message = |key| {
        bundle
            .messages
            .iter()
            .find_map(|(candidate, message)| (candidate == key).then_some(message.text.as_str()))
    };
    let Some(product_name) = message("ui.product.name") else {
        return registration_failed();
    };
    if settings_unavailable || selection.stored_unavailable {
        eprintln!("settings_unavailable");
    }
    for warning in registry.integrity_warnings() {
        eprintln!(
            "{warning}: {}",
            message(warning.as_str()).unwrap_or(warning)
        );
    }
    match args.command {
        args::Command::Help => {
            let Some(usage) = message("cli.help.usage") else {
                return registration_failed();
            };
            println!("{product_name}\n\n{usage}");
        }
        args::Command::Version => {
            println!("{product_name} {}", env!("CARGO_PKG_VERSION"));
        }
        // Scan admission is parsed, but execution awaits the engine and policy composition.
        args::Command::Scan(_) | args::Command::Unsupported => {
            let Some(unsupported) = message("cli.help.unsupported") else {
                return registration_failed();
            };
            eprintln!("{unsupported}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

fn unsupported_language() -> ExitCode {
    eprintln!("DiskUnfold: locale.unsupported");
    ExitCode::from(2)
}

fn registration_failed() -> ExitCode {
    eprintln!("DiskUnfold: locale.registration_failed");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
