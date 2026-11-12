use std::ffi::{OsStr, OsString};

pub(crate) enum Command {
    Help,
    Version,
    Notices(NoticeDocument),
    Support(OutputFormat),
    /// A syntactically admitted scan; execution awaits engine and policy composition.
    Scan(Scan),
    /// Maps to F39 FR6's `command.not_provided` state; no machine output is wired yet.
    NotProvided,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoticeDocument {
    License,
    ThirdParty,
}

enum CommandBuilder {
    Help,
    Version,
    Notices(Option<NoticeDocument>),
    Support,
    Scan(Scan),
    NotProvided,
    Unsupported,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Scan {
    roots: Vec<OsString>,
    excludes: Vec<OsString>,
    device: Option<Device>,
}

#[derive(Debug, PartialEq, Eq)]
enum Device {
    Auto,
    Ssd,
    Hdd,
}

const DENIED_OPTION_NAMES: &[&str] = &[
    "--approve",
    "--execute",
    "--trash",
    "--delete",
    "--move",
    "--restore",
    "--undo",
    "--protect",
    "--mcp",
    "--key",
];

pub(crate) struct Arguments {
    pub(crate) command: Command,
    pub(crate) language: Option<String>,
}

#[derive(Default)]
struct ParseState {
    language: Option<String>,
    format: Option<OutputFormat>,
    command: Option<CommandBuilder>,
    options_ended: bool,
    invalid: bool,
}

pub(crate) fn parse(mut arguments: impl Iterator<Item = OsString>) -> Option<Arguments> {
    let mut state = ParseState::default();
    while let Some(argument) = arguments.next() {
        consume_argument(&mut state, argument, &mut arguments)?;
    }
    let command = match state.command {
        Some(CommandBuilder::NotProvided) => Command::NotProvided,
        Some(CommandBuilder::Scan(scan))
            if !state.invalid && state.format.is_none() && !scan.roots.is_empty() =>
        {
            Command::Scan(scan)
        }
        Some(CommandBuilder::Help)
            if !state.invalid && !state.options_ended && state.format.is_none() =>
        {
            Command::Help
        }
        Some(CommandBuilder::Version)
            if !state.invalid && !state.options_ended && state.format.is_none() =>
        {
            Command::Version
        }
        Some(CommandBuilder::Notices(Some(document)))
            if !state.invalid && !state.options_ended && state.format.is_none() =>
        {
            Command::Notices(document)
        }
        Some(CommandBuilder::Support) if !state.invalid && !state.options_ended => {
            Command::Support(state.format.unwrap_or(OutputFormat::Text))
        }
        _ => Command::Unsupported,
    };
    Some(Arguments {
        command,
        language: state.language,
    })
}

fn consume_argument(
    state: &mut ParseState,
    argument: OsString,
    arguments: &mut impl Iterator<Item = OsString>,
) -> Option<()> {
    if consume_global_argument(state, &argument, arguments)? {
        return Some(());
    }
    if state.command.is_none() {
        state.command = Some(if state.options_ended {
            state.invalid = true;
            CommandBuilder::Unsupported
        } else {
            parse_command(argument)
        });
    } else {
        consume_command_argument(state, argument, arguments);
    }
    Some(())
}

fn consume_global_argument(
    state: &mut ParseState,
    argument: &OsStr,
    arguments: &mut impl Iterator<Item = OsString>,
) -> Option<bool> {
    if state.options_ended {
        return Some(false);
    }
    if is_denied_option(argument) {
        state.command = Some(CommandBuilder::NotProvided);
        return Some(true);
    }
    if argument == "--lang" {
        if state.language.is_some() {
            return None;
        }
        state.language = Some(arguments.next()?.into_string().ok()?);
        return Some(true);
    }
    if argument == "--format" {
        let parsed = match arguments.next()?.to_str()? {
            "text" => Some(OutputFormat::Text),
            "json" => Some(OutputFormat::Json),
            _ => None,
        };
        if state.format.is_some() {
            state.invalid = true;
        } else {
            state.format = parsed;
            state.invalid |= state.format.is_none();
        }
        return Some(true);
    }
    if argument == "--" {
        state.options_ended = true;
        return Some(true);
    }
    Some(false)
}

fn consume_command_argument(
    state: &mut ParseState,
    argument: OsString,
    arguments: &mut impl Iterator<Item = OsString>,
) {
    if let Some(command) = &mut state.command {
        match command {
            CommandBuilder::Scan(scan) => {
                if !scan_argument(scan, argument, state.options_ended, arguments) {
                    state.invalid = true;
                }
            }
            CommandBuilder::Notices(document @ None) => {
                *document = parse_notice_document(&argument);
                state.invalid |= document.is_none();
            }
            _ => state.invalid = true,
        }
    }
}

fn parse_command(argument: OsString) -> CommandBuilder {
    match argument.to_str() {
        Some("--help" | "-h") => CommandBuilder::Help,
        Some("--version" | "-V" | "version") => CommandBuilder::Version,
        Some("notices") => CommandBuilder::Notices(None),
        Some("support") => CommandBuilder::Support,
        Some("scan") => CommandBuilder::Scan(Scan::default()),
        _ if is_option(&argument) => CommandBuilder::Unsupported,
        _ => CommandBuilder::NotProvided,
    }
}

fn parse_notice_document(value: &OsStr) -> Option<NoticeDocument> {
    match value.to_str()? {
        "LICENSE" => Some(NoticeDocument::License),
        "THIRD-PARTY-NOTICES" => Some(NoticeDocument::ThirdParty),
        _ => None,
    }
}

fn scan_argument(
    scan: &mut Scan,
    argument: OsString,
    options_ended: bool,
    arguments: &mut impl Iterator<Item = OsString>,
) -> bool {
    if !options_ended && argument == "--exclude" {
        return match arguments.next() {
            Some(value) if !value.is_empty() && !is_option(&value) => {
                scan.excludes.push(value);
                true
            }
            _ => false,
        };
    }
    if !options_ended && argument == "--device" {
        let value = arguments.next();
        if scan.device.is_some() {
            return false;
        }
        scan.device = match value.as_deref().and_then(OsStr::to_str) {
            Some("auto") => Some(Device::Auto),
            Some("ssd") => Some(Device::Ssd),
            Some("hdd") => Some(Device::Hdd),
            _ => None,
        };
        return scan.device.is_some();
    }
    if argument.is_empty() || (!options_ended && is_option(&argument)) {
        return false;
    }
    scan.roots.push(argument);
    true
}

fn is_option(value: &OsStr) -> bool {
    value.as_encoded_bytes().starts_with(b"-")
}

fn is_denied_option(value: &OsStr) -> bool {
    value
        .to_str()
        .is_some_and(|value| DENIED_OPTION_NAMES.contains(&value))
}

#[cfg(test)]
mod tests;
