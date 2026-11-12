use std::ffi::{OsStr, OsString};

pub(crate) enum Command {
    Help,
    Version,
    /// A syntactically admitted scan; execution awaits engine and policy composition.
    Scan(Scan),
    /// Maps to F39 FR6's `command.not_provided` state; no machine output is wired yet.
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

pub(crate) struct Arguments {
    pub(crate) command: Command,
    pub(crate) language: Option<String>,
}

#[derive(Default)]
struct ParseState {
    language: Option<String>,
    command: Option<Command>,
    options_ended: bool,
    invalid: bool,
}

pub(crate) fn parse(mut arguments: impl Iterator<Item = OsString>) -> Option<Arguments> {
    let mut state = ParseState::default();
    while let Some(argument) = arguments.next() {
        consume_argument(&mut state, argument, &mut arguments)?;
    }
    let command = match state.command {
        Some(Command::NotProvided) => Command::NotProvided,
        Some(Command::Scan(scan)) if !state.invalid && !scan.roots.is_empty() => {
            Command::Scan(scan)
        }
        Some(Command::Help) if !state.invalid && !state.options_ended => Command::Help,
        Some(Command::Version) if !state.invalid && !state.options_ended => Command::Version,
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
    if !state.options_ended && argument == "--lang" {
        if state.language.is_some() {
            return None;
        }
        state.language = Some(arguments.next()?.into_string().ok()?);
    } else if !state.options_ended && argument == "--" {
        state.options_ended = true;
    } else if state.command.is_none() {
        if state.options_ended {
            // Do not reinterpret values after a leading marker as commands.
            state.invalid = true;
        } else {
            state.command = Some(parse_command(argument));
        }
    } else if let Some(Command::Scan(scan)) = &mut state.command {
        if !scan_argument(scan, argument, state.options_ended, arguments) {
            state.invalid = true;
        }
    } else {
        state.invalid = true;
    }
    Some(())
}

fn parse_command(argument: OsString) -> Command {
    match argument.to_str() {
        Some("--help" | "-h") => Command::Help,
        Some("--version" | "-V") => Command::Version,
        Some("scan") => Command::Scan(Scan::default()),
        _ if is_option(&argument) => Command::Unsupported,
        _ => Command::NotProvided,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &[&str]) -> Option<Arguments> {
        parse(input.iter().map(OsString::from))
    }

    #[test]
    fn fr1_scan_paths_and_end_marker() -> Result<(), String> {
        let Some(Arguments {
            command: Command::Scan(scan),
            language,
        }) = parsed(&[
            "scan",
            "--exclude",
            "ignored",
            "--lang",
            "en",
            "--device",
            "ssd",
            "--",
            "-root",
            "--lang",
        ])
        else {
            return Err("expected scan".into());
        };
        assert_eq!(language.as_deref(), Some("en"));
        assert_eq!(
            scan.roots,
            [OsString::from("-root"), OsString::from("--lang")]
        );
        assert_eq!(scan.excludes, [OsString::from("ignored")]);
        assert_eq!(scan.device, Some(Device::Ssd));
        Ok(())
    }

    #[test]
    fn fr6_fr7_option_values_are_not_global_options() {
        for input in [
            vec!["scan", "root", "--exclude", "--lang", "en"],
            vec!["scan", "root", "--device", "--lang", "en"],
            vec!["scan"],
            vec!["scan", "root", "--device", "ssd", "--device", "hdd"],
            vec!["--", "scan"],
        ] {
            assert!(matches!(
                parsed(&input),
                Some(Arguments {
                    command: Command::Unsupported,
                    ..
                })
            ));
        }
        assert!(matches!(
            parsed(&["--lang", "--execute"]),
            Some(Arguments {
                command: Command::Unsupported,
                ..
            })
        ));
    }

    #[test]
    fn fr6_unregistered_and_forbidden_command_names_are_not_provided() -> Result<(), String> {
        let Some(Arguments { command, .. }) = parsed(&["unknown-command"]) else {
            return Err("unknown command did not parse".into());
        };
        assert!(matches!(&command, Command::NotProvided));

        for name in [
            "approve", "execute", "trash", "delete", "move", "restore", "undo", "protect", "mcp",
            "key",
        ] {
            let Some(Arguments { command, .. }) = parse([OsString::from(name)].into_iter()) else {
                return Err(format!("forbidden command {name} did not parse"));
            };
            assert!(matches!(&command, Command::NotProvided), "{name}");
        }
        Ok(())
    }

    #[test]
    fn fr6_unregistered_commands_are_not_provided() {
        for input in [
            vec!["duplicates"],
            vec!["status"],
            vec!["query"],
            vec!["compare"],
            vec!["report"],
            vec!["export"],
            vec!["plan"],
            vec!["cancel"],
            vec!["session"],
        ] {
            assert!(
                matches!(
                    parsed(&input),
                    Some(Arguments {
                        command: Command::NotProvided,
                        ..
                    })
                ),
                "{input:?}"
            );
        }
    }

    #[test]
    fn fr6_malformed_registered_commands_and_unknown_options_keep_usage_classification() {
        for input in [
            vec!["--help", "unexpected"],
            vec!["--help", "--execute"],
            vec!["scan"],
            vec!["scan", "root", "--unknown"],
            vec!["scan", "root", "--execute"],
            vec!["--bogus"],
            vec!["-x"],
            vec!["scan", "--exclude", "--execute", "root"],
            vec!["scan", "--device", "--delete", "root"],
        ] {
            assert!(
                matches!(
                    parsed(&input),
                    Some(Arguments {
                        command: Command::Unsupported,
                        ..
                    })
                ),
                "{input:?}"
            );
        }
        assert!(matches!(
            parsed(&["scan", "root", "--", "--execute"]),
            Some(Arguments {
                command: Command::Scan(_),
                ..
            })
        ));
        assert!(matches!(
            parsed(&["--", "--execute"]),
            Some(Arguments {
                command: Command::Unsupported,
                ..
            })
        ));
        assert!(matches!(
            parsed(&["scan", "delete"]),
            Some(Arguments {
                command: Command::Scan(_),
                ..
            })
        ));
        assert!(matches!(
            parsed(&["scan", "--exclude", "delete", "root"]),
            Some(Arguments {
                command: Command::Scan(_),
                ..
            })
        ));
    }

    #[test]
    fn locale_and_help_version_remain_available() {
        assert!(matches!(
            parsed(&["--lang", "en", "--help"]),
            Some(Arguments {
                command: Command::Help,
                ..
            })
        ));
        assert!(matches!(
            parsed(&["-V", "--lang", "en"]),
            Some(Arguments {
                command: Command::Version,
                ..
            })
        ));
        assert!(parsed(&["--lang", "en", "--lang", "ko"]).is_none());
    }

    #[test]
    fn scan_options_and_roots_preserve_order() -> Result<(), String> {
        let Some(Arguments {
            command: Command::Scan(scan),
            ..
        }) = parsed(&[
            "scan",
            "first",
            "--exclude",
            "one",
            "second",
            "--exclude",
            "two",
            "--device",
            "auto",
        ])
        else {
            return Err("expected scan".into());
        };
        assert_eq!(
            scan.roots,
            [OsString::from("first"), OsString::from("second")]
        );
        assert_eq!(
            scan.excludes,
            [OsString::from("one"), OsString::from("two")]
        );
        assert_eq!(scan.device, Some(Device::Auto));
        Ok(())
    }

    #[test]
    fn scan_rejects_missing_and_invalid_values() {
        for input in [
            vec!["scan", "root", "--exclude"],
            vec!["scan", "root", "--exclude", ""],
            vec!["scan", "root", "--device"],
            vec!["scan", "root", "--device", "unknown"],
            vec!["scan", "-root"],
            vec!["scan", "--"],
            vec!["scan", ""],
        ] {
            assert!(matches!(
                parsed(&input),
                Some(Arguments {
                    command: Command::Unsupported,
                    ..
                })
            ));
        }
    }

    #[cfg(windows)]
    #[test]
    fn fr1_preserves_non_unicode_windows_paths() -> Result<(), String> {
        use std::os::windows::ffi::OsStringExt;
        let root = OsString::from_wide(&[0xd800, 0x61]);
        let Some(Arguments {
            command: Command::Scan(scan),
            ..
        }) = parse([OsString::from("scan"), root.clone()].into_iter())
        else {
            return Err("expected scan".into());
        };
        assert_eq!(scan.roots, [root]);
        Ok(())
    }
}
