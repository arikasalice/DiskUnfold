use std::ffi::OsString;

pub(crate) enum Command {
    Help,
    Version,
    Unsupported,
}

pub(crate) struct Arguments {
    pub(crate) command: Command,
    pub(crate) language: Option<String>,
}

pub(crate) fn parse(mut arguments: impl Iterator<Item = OsString>) -> Option<Arguments> {
    let mut language = None;
    let mut command = None;
    while let Some(argument) = arguments.next() {
        if argument == "--lang" {
            if language.is_some() {
                return None;
            }
            language = Some(arguments.next()?.into_string().ok()?);
        } else if command.is_some() {
            command = Some(Command::Unsupported);
        } else {
            command = Some(match argument.to_str() {
                Some("--help" | "-h") => Command::Help,
                Some("--version" | "-V") => Command::Version,
                _ => Command::Unsupported,
            });
        }
    }
    Some(Arguments {
        command: command.unwrap_or(Command::Unsupported),
        language,
    })
}
