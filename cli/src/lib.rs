//! Parse CLI arguments and present engine results.

use std::{
    ffi::OsString,
    io::{self, Write},
    process::ExitCode,
};

use du_core_contracts::product_support::{
    EmbeddedProductInfo, SupportEnvironmentPort, SupportMatrix,
};
use du_engine_api::support;
use du_platform_os::support_environment::SystemSupportEnvironment;
use du_user_env::{i18n::embedded, locale::DecisionError};

mod args;
mod product_output;
mod startup_locale;

const SETTINGS_UNAVAILABLE_REASON_KEY: &str = "settings_unavailable";
const SETTINGS_UNAVAILABLE_MESSAGE_KEY: &str = "cli.settings.unavailable";

pub fn run(arguments: impl Iterator<Item = OsString>) -> ExitCode {
    run_with_product_info(arguments, default_product_info())
}

/// Runs the CLI with product resources supplied by executable assembly.
///
/// Product information is passed from the launcher as embedded data; this
/// entry point does not open the application database or inspect the network.
pub fn run_with_product_info(
    arguments: impl Iterator<Item = OsString>,
    product_info: EmbeddedProductInfo,
) -> ExitCode {
    #[cfg(all(windows, feature = "windows-console"))]
    du_platform_os::windows_console::attach_parent();

    run_with_product(
        arguments,
        product_info,
        &SystemSupportEnvironment,
        startup_locale::stored_language,
        startup_locale::preferred_ui_languages,
    )
}

/// Runs with the exact product metadata JSON embedded by the launcher.
pub fn run_with_product_info_json(
    arguments: impl Iterator<Item = OsString>,
    product_info_json: &str,
) -> ExitCode {
    match serde_json::from_str(product_info_json) {
        Ok(product_info) => run_with_product_info(arguments, product_info),
        Err(_) => {
            eprintln!("DiskUnfold: product.metadata_invalid");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
fn run_with(
    arguments: impl Iterator<Item = OsString>,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
) -> ExitCode {
    run_with_product(
        arguments,
        default_product_info(),
        &SystemSupportEnvironment,
        read_stored,
        read_os,
    )
}

fn run_with_product(
    arguments: impl Iterator<Item = OsString>,
    product_info: EmbeddedProductInfo,
    environment: &impl SupportEnvironmentPort,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
) -> ExitCode {
    let stdout = io::stdout();
    let stderr = io::stderr();
    run_with_io_and_product_info(
        arguments,
        product_info,
        environment,
        read_stored,
        read_os,
        &mut stdout.lock(),
        &mut stderr.lock(),
    )
}

/// Runs a parsed CLI request against explicit streams for error handling tests.
#[cfg(test)]
fn run_with_io<W: Write, E: Write>(
    arguments: impl Iterator<Item = OsString>,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
    stdout: &mut W,
    stderr: &mut E,
) -> ExitCode {
    run_with_io_and_product_info(
        arguments,
        default_product_info(),
        &SystemSupportEnvironment,
        read_stored,
        read_os,
        stdout,
        stderr,
    )
}

fn run_with_io_and_product_info<W: Write, E: Write>(
    arguments: impl Iterator<Item = OsString>,
    product_info: EmbeddedProductInfo,
    environment: &impl SupportEnvironmentPort,
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
    let skip_stored_settings = matches!(
        &args.command,
        args::Command::Help
            | args::Command::Version
            | args::Command::Notices(_)
            | args::Command::Support(_)
            | args::Command::NotProvided
            | args::Command::Unsupported
    );
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
    let message = product_output::Messages::new(&registry, &selection.language);
    let Some(product_name) = message.resolve("ui.product.name") else {
        write_stderr_line(stderr, "DiskUnfold: locale.registration_failed");
        return ExitCode::FAILURE;
    };
    if settings_unavailable || selection.stored_unavailable {
        write_settings_unavailable_warning(
            stderr,
            message.resolve(SETTINGS_UNAVAILABLE_MESSAGE_KEY).as_deref(),
        );
    }
    for warning in registry.integrity_warnings() {
        let warning = format!(
            "{warning}: {}",
            message
                .resolve(warning.as_str())
                .unwrap_or_else(|| warning.clone())
        );
        write_stderr_line(stderr, &warning);
    }
    let output = match args.command {
        args::Command::Help => {
            let Some(usage) = message.resolve("cli.help.usage") else {
                write_stderr_line(stderr, "DiskUnfold: locale.registration_failed");
                return ExitCode::FAILURE;
            };
            write!(stdout, "{product_name}\n\n{usage}\n")
        }
        args::Command::Version => product_output::write_version(stdout, &message, &product_info),
        args::Command::Notices(document) => {
            match product_output::notice_text(document, &product_info) {
                Some(notice) => write!(stdout, "{notice}"),
                None => {
                    let text = message
                        .resolve("cli.notices.unavailable")
                        .unwrap_or_else(|| "cli.notices.unavailable".to_owned());
                    write_stderr_line(stderr, &text);
                    return ExitCode::from(1);
                }
            }
        }
        args::Command::Support(format) => {
            let matrix = product_info
                .support_matrix
                .as_ref()
                .unwrap_or(&EMPTY_MATRIX);
            let report =
                support::evaluate(matrix, environment.identity(), &environment.capabilities());
            match format {
                args::OutputFormat::Json => product_output::write_support_json(stdout, &report),
                args::OutputFormat::Text => {
                    product_output::write_support_text(stdout, &message, &report)
                }
            }
        }
        // Scan admission is parsed, but execution awaits the engine and policy composition.
        args::Command::Scan(scan) => {
            drop(scan);
            return write_unsupported(stderr, message.resolve("cli.help.unsupported"));
        }
        args::Command::NotProvided | args::Command::Unsupported => {
            return write_unsupported(stderr, message.resolve("cli.help.unsupported"));
        }
    };
    if output.is_err() {
        write_output_interrupted_diagnostic(
            stderr,
            message.resolve("cli.output.interrupted").as_deref(),
        );
    }
    output_exit_code(output)
}

fn write_unsupported(stderr: &mut impl Write, localized: Option<String>) -> ExitCode {
    let Some(unsupported) = localized else {
        write_stderr_line(stderr, "DiskUnfold: locale.registration_failed");
        return ExitCode::FAILURE;
    };
    write_stderr_line(stderr, &unsupported);
    ExitCode::from(2)
}

static EMPTY_MATRIX: SupportMatrix = SupportMatrix {
    revision: None,
    as_of: None,
    combinations: Vec::new(),
};

fn default_product_info() -> EmbeddedProductInfo {
    product_output::default_product_info(env!("CARGO_PKG_VERSION"))
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
mod tests;

#[cfg(test)]
mod validation_tests;

#[cfg(test)]
mod product_tests;
