//! Parse CLI arguments and present engine results.

use std::{ffi::OsStr, process::ExitCode};

use du_user_env::i18n::embedded;

pub fn run(arg: &OsStr, extra_args: bool) -> ExitCode {
    #[cfg(all(windows, feature = "windows-console"))]
    du_platform_os::windows_console::attach_parent();

    let Ok(registry) = embedded::register(&[]) else {
        return registration_failed();
    };
    let Some(english) = registry.bundle("en") else {
        return registration_failed();
    };
    let message = |key| {
        english
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
    match (arg.to_str(), extra_args) {
        (Some("--help" | "-h"), false) => {
            let Some(usage) = message("cli.help.usage") else {
                return registration_failed();
            };
            println!("{product_name}\n\n{usage}");
        }
        (Some("--version" | "-V"), false) => {
            println!("{product_name} {}", env!("CARGO_PKG_VERSION"));
        }
        _ => {
            let Some(unsupported) = message("cli.help.unsupported") else {
                return registration_failed();
            };
            eprintln!("{unsupported}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

fn registration_failed() -> ExitCode {
    eprintln!("DiskUnfold: locale.registration_failed");
    ExitCode::FAILURE
}
