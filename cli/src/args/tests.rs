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
        "approve", "execute", "trash", "delete", "move", "restore", "undo", "protect", "mcp", "key",
    ] {
        let Some(Arguments { command, .. }) = parse([OsString::from(name)].into_iter()) else {
            return Err(format!("forbidden command {name} did not parse"));
        };
        assert!(matches!(&command, Command::NotProvided), "{name}");
    }
    Ok(())
}

#[test]
fn fr6_exact_denied_options_are_not_provided_only_in_active_positions() -> Result<(), String> {
    for name in [
        "approve", "execute", "trash", "delete", "move", "restore", "undo", "protect", "mcp", "key",
    ] {
        for input in [
            vec![format!("--{name}")],
            vec!["scan".to_owned(), format!("--{name}")],
            vec![format!("--{name}"), "scan".to_owned()],
            vec!["--help".to_owned(), format!("--{name}")],
        ] {
            let Some(Arguments { command, .. }) = parse(input.into_iter().map(OsString::from))
            else {
                return Err(format!("{name} did not parse"));
            };
            if !matches!(command, Command::NotProvided) {
                return Err(format!("{name} was not classified as not-provided"));
            }
        }
    }

    for input in [
        vec!["scan", "root", "--exclude", "--delete"],
        vec!["scan", "root", "--device", "--execute"],
    ] {
        if !matches!(
            parsed(&input),
            Some(Arguments {
                command: Command::Unsupported,
                ..
            })
        ) {
            return Err(format!(
                "consumed option value was reinterpreted: {input:?}"
            ));
        }
    }

    let Some(Arguments {
        command: Command::Scan(scan),
        language,
    }) = parsed(&["--lang", "--execute", "scan", "root"])
    else {
        return Err("the --lang value must be consumed before command parsing".into());
    };
    assert_eq!(language.as_deref(), Some("--execute"));
    assert_eq!(scan.roots, [OsString::from("root")]);

    let Some(Arguments {
        command: Command::Scan(scan),
        ..
    }) = parsed(&["scan", "--", "--delete"])
    else {
        return Err("a path after -- must not be reinterpreted as an option".into());
    };
    assert_eq!(scan.roots, [OsString::from("--delete")]);

    for input in [vec!["--del"], vec!["--delete=value"], vec!["--execute-now"]] {
        if !matches!(
            parsed(&input),
            Some(Arguments {
                command: Command::Unsupported,
                ..
            })
        ) {
            return Err(format!("non-exact option was treated as denied: {input:?}"));
        }
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
        vec!["scan"],
        vec!["scan", "root", "--unknown"],
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
fn f15_notice_and_support_commands_accept_only_published_arguments() {
    assert!(matches!(
        parsed(&["notices", "LICENSE"]),
        Some(Arguments {
            command: Command::Notices(NoticeDocument::License),
            ..
        })
    ));
    assert!(matches!(
        parsed(&["--lang", "ko", "support", "--format", "json"]),
        Some(Arguments {
            command: Command::Support(OutputFormat::Json),
            ..
        })
    ));
    assert!(matches!(
        parsed(&["--format", "json", "support"]),
        Some(Arguments {
            command: Command::Support(OutputFormat::Json),
            ..
        })
    ));
    for input in [
        vec!["notices"],
        vec!["notices", "COPYING"],
        vec!["notices", "LICENSE", "unexpected"],
        vec!["support", "json"],
        vec!["support", "--format", "yaml"],
        vec!["support", "--format", "json", "--format", "text"],
        vec!["--format", "json", "--version"],
        vec!["--format", "text", "--help"],
        vec!["--format", "text", "notices", "LICENSE"],
        vec!["--format", "text", "scan", "root"],
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
