//! Parse CLI arguments and present engine results.

use std::{
    ffi::OsString,
    io::{self, Write},
    process::ExitCode,
};

use du_core_contracts::ErrorCode;
use du_core_contracts::product_support::{
    EmbeddedProductInfo, SupportEnvironmentPort, SupportMatrix,
};
use du_engine_api::support;
use du_platform_os::{
    startup::{FullDiskAccessStatus, StartupCapabilitiesPort, SystemStartupCapabilities},
    support_environment::SystemSupportEnvironment,
};
use du_user_env::{
    i18n::{Registry, embedded},
    locale::DecisionError,
};

mod args;
mod diagnostics;
mod machine_output;
mod product_output;
mod startup_locale;

use diagnostics::{write_error_line, write_stderr_line};

const SETTINGS_UNAVAILABLE_REASON_KEY: &str = "settings_unavailable";
const SETTINGS_UNAVAILABLE_MESSAGE_KEY: &str = "cli.settings.unavailable";
/// Stable reason key written when analysis is refused for missing full disk access.
const FULL_DISK_ACCESS_REQUIRED_REASON_KEY: &str = "startup.full_disk_access_required";
/// Locale message that explains the full disk access denial to the user.
const FULL_DISK_ACCESS_REQUIRED_MESSAGE_KEY: &str = "cli.access.full_disk_access_required";
/// Exit code for argument errors and for a confirmed full disk access denial.
const EXIT_USAGE: u8 = 2;
/// Exit code for a technical failure or unavailable embedded data.
const EXIT_FAILURE: u8 = 1;

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
        &SystemStartupCapabilities,
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
        &SystemStartupCapabilities,
        read_stored,
        read_os,
    )
}

fn run_with_product(
    arguments: impl Iterator<Item = OsString>,
    product_info: EmbeddedProductInfo,
    environment: &impl SupportEnvironmentPort,
    access: &impl StartupCapabilitiesPort,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
) -> ExitCode {
    let stdout = io::stdout();
    let stderr = io::stderr();
    run_with_io_and_product_info(
        arguments,
        product_info,
        &OsPorts {
            environment,
            access,
        },
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
        &OsPorts {
            environment: &SystemSupportEnvironment,
            access: &SystemStartupCapabilities,
        },
        read_stored,
        read_os,
        stdout,
        stderr,
    )
}

/// Operating-system ports consulted by the CLI commands.
struct OsPorts<'a, E, A> {
    /// Environment identity and capabilities for `support`.
    environment: &'a E,
    /// Full disk access observation for commands that start analysis (IF-09).
    access: &'a A,
}

/// Runs the CLI with injected product, environment and access ports.
///
/// The information commands (`version`, `notices`, `support`) never open the
/// engine or the application database. The access port is consulted only by
/// commands that would start analysis, before any job exists.
fn run_with_io_and_product_info<W: Write, E: Write>(
    arguments: impl Iterator<Item = OsString>,
    product_info: EmbeddedProductInfo,
    ports: &OsPorts<'_, impl SupportEnvironmentPort, impl StartupCapabilitiesPort>,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
    stdout: &mut W,
    stderr: &mut E,
) -> ExitCode {
    let args = args::parse(arguments);
    let registry = embedded::register(&[]);
    let Some(args) = args else {
        write_locale_unsupported(stderr, registry.as_ref().ok());
        return ExitCode::from(2);
    };
    let Ok(registry) = registry else {
        write_registration_failed(stderr);
        return ExitCode::FAILURE;
    };
    run_with_registry(
        Parsed {
            args,
            registry: &registry,
        },
        product_info,
        ports,
        read_stored,
        read_os,
        stdout,
        stderr,
    )
}

/// Runs a parsed request against a registered language registry.
///
/// After the command finishes, each message that used a fallback is reported
/// once on stderr as `locale.translation_fallback`. The report never changes
/// the exit code.
fn run_with_registry<W: Write, E: Write>(
    parsed: Parsed<'_>,
    product_info: EmbeddedProductInfo,
    ports: &OsPorts<'_, impl SupportEnvironmentPort, impl StartupCapabilitiesPort>,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
    stdout: &mut W,
    stderr: &mut E,
) -> ExitCode {
    let registry = parsed.registry;
    let exit = run_command(
        parsed,
        product_info,
        ports,
        read_stored,
        read_os,
        stdout,
        stderr,
    );
    diagnostics::write_translation_fallbacks(stderr, registry);
    exit
}

/// A parsed request together with the registry that resolves its messages.
struct Parsed<'a> {
    args: args::Arguments,
    registry: &'a Registry,
}

/// Selects the language and runs one command.
fn run_command<W: Write, E: Write>(
    parsed: Parsed<'_>,
    product_info: EmbeddedProductInfo,
    ports: &OsPorts<'_, impl SupportEnvironmentPort, impl StartupCapabilitiesPort>,
    read_stored: impl FnOnce() -> Result<Option<String>, ()>,
    read_os: impl FnOnce() -> Option<Vec<String>>,
    stdout: &mut W,
    stderr: &mut E,
) -> ExitCode {
    let Parsed { args, registry } = parsed;
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
        registry,
        args.language.as_deref(),
        skip_stored_settings,
        read_stored,
        read_os,
    ) {
        Ok(selection) => selection,
        Err(DecisionError::UnsupportedLanguage) => {
            write_locale_unsupported(stderr, Some(registry));
            return ExitCode::from(2);
        }
        Err(DecisionError::RegistrationFailed) => {
            write_registration_failed(stderr);
            return ExitCode::FAILURE;
        }
    };
    let message = product_output::Messages::new(registry, &selection.language);
    let Some(product_name) = message.resolve("ui.product.name") else {
        write_registration_failed(stderr);
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
                write_registration_failed(stderr);
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
                    return ExitCode::from(EXIT_FAILURE);
                }
            }
        }
        args::Command::Support(format) => {
            let matrix = product_output::consistent_matrix(&product_info).unwrap_or(&EMPTY_MATRIX);
            let report = support::evaluate(
                matrix,
                ports.environment.identity(),
                &ports.environment.capabilities(),
            );
            match format {
                // The machine renderer receives the raw report only: no resolver, no format profile.
                args::OutputFormat::Json => machine_output::write_support_json(stdout, &report),
                args::OutputFormat::Text => {
                    product_output::write_support_text(stdout, &message, &report)
                }
            }
        }
        // Scan admission is parsed, but execution awaits the engine and policy composition.
        // The access check precedes any job, so a confirmed denial never creates one.
        args::Command::Scan(scan) => {
            drop(scan);
            if ports.access.full_disk_access_status() == FullDiskAccessStatus::Denied {
                // One line: reason key, localized explanation, English error code.
                let text = message
                    .resolve(FULL_DISK_ACCESS_REQUIRED_MESSAGE_KEY)
                    .unwrap_or_else(|| FULL_DISK_ACCESS_REQUIRED_MESSAGE_KEY.to_owned());
                write_error_line(
                    stderr,
                    FULL_DISK_ACCESS_REQUIRED_REASON_KEY,
                    &ErrorCode::InvalidArgument,
                    Some(&text),
                );
                return ExitCode::from(EXIT_USAGE);
            }
            return write_unsupported(stderr, message.resolve("cli.help.unsupported"));
        }
        args::Command::NotProvided | args::Command::Unsupported => {
            return write_unsupported(stderr, message.resolve("cli.help.unsupported"));
        }
    };
    // A buffered tail (for example a notice without a final newline) must reach
    // the stream before a closed stdout can be reported as exit code 5.
    let output = output.and_then(|()| stdout.flush());
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
        write_registration_failed(stderr);
        return ExitCode::FAILURE;
    };
    write_stderr_line(stderr, &unsupported);
    ExitCode::from(2)
}

/// Writes the `locale.unsupported` error line, in English because the
/// requested language is not available. Without a registry only the key and
/// code are written.
fn write_locale_unsupported(stderr: &mut impl Write, registry: Option<&Registry>) {
    let text = registry
        .and_then(|registry| registry.resolve_plain_text("en", "reason.locale.unsupported.cause"));
    write_error_line(
        stderr,
        "locale.unsupported",
        &ErrorCode::InvalidArgument,
        text.as_deref(),
    );
}

/// Writes the `locale.registration_failed` error line; no text can be resolved.
fn write_registration_failed(stderr: &mut impl Write) {
    write_error_line(
        stderr,
        "locale.registration_failed",
        &ErrorCode::Internal,
        None,
    );
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

/// Expected `locale.unsupported` error line: reason key, English cause, English error code.
#[cfg(test)]
const LOCALE_UNSUPPORTED_LINE: &[u8] =
    b"DiskUnfold: locale.unsupported: The requested language is unavailable. [INVALID_ARGUMENT]\n";

#[cfg(test)]
mod validation_tests;

#[cfg(test)]
mod fallback_tests;

#[cfg(test)]
mod product_tests;

#[cfg(test)]
mod readonly_database_tests;
