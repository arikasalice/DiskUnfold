//! Parse CLI arguments and present engine results.

use std::{ffi::OsString, process::ExitCode};

use du_user_env::{
    i18n::embedded,
    locale::{self, DecisionError, Inputs, OsLanguage},
};

mod args;

pub fn run(arguments: impl Iterator<Item = OsString>) -> ExitCode {
    #[cfg(all(windows, feature = "windows-console"))]
    du_platform_os::windows_console::attach_parent();

    let Some(args) = args::parse(arguments) else {
        return unsupported_language();
    };
    let Ok(registry) = embedded::register(&[]) else {
        return registration_failed();
    };
    let registered: Vec<_> = registry.languages().collect();
    let selection = locale::decide_cli(
        &Inputs {
            registered: &registered,
            stored: None,
            os: OsLanguage::Unavailable,
            korean_integrity_failed: !registry.integrity_warnings().is_empty(),
        },
        args.language.as_deref(),
    );
    let language = match selection {
        Ok(selection) => selection.language,
        Err(DecisionError::UnsupportedLanguage) => return unsupported_language(),
        Err(DecisionError::RegistrationFailed) => return registration_failed(),
    };
    let Some(bundle) = registry.bundle(&language) else {
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
        args::Command::Unsupported => {
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
