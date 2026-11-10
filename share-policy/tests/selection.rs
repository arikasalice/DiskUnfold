use du_share_policy::selection::{Error, Mode, Recipient, select};

#[test]
fn analysis_channels_default_to_aggregate_without_remembering_previous_selection() {
    for channel in ["mcp", "cli", "report", "data-export", "diagnostics"] {
        let selected = select(channel, None, None, None);
        assert!(
            matches!(selected, Ok(value) if value.mode == Mode::AggregateOnly && value.defaulted)
        );
        assert!(select(channel, Some(Mode::Pseudonymize), None, None).is_ok());
        assert!(
            matches!(select(channel, None, None, None), Ok(value) if value.mode == Mode::AggregateOnly && value.defaulted)
        );
    }
}

#[test]
fn settings_require_explicit_path_include_and_diagnostics_disallow_it() {
    assert_eq!(
        select("settings", None, None, None),
        Err(Error::ModeNotAllowed)
    );
    assert!(
        matches!(select("settings", Some(Mode::PathInclude), None, None), Ok(value) if value.mode == Mode::PathInclude && !value.defaulted)
    );
    assert_eq!(
        select(
            "settings",
            Some(Mode::PathInclude),
            Some(Mode::AggregateOnly),
            None
        ),
        Err(Error::ModeNotAllowed)
    );
    assert_eq!(
        select("diagnostics", Some(Mode::PathInclude), None, None),
        Err(Error::ModeNotAllowed)
    );
}

#[test]
fn session_limit_is_stricter_and_output_options_cannot_expand_it() {
    let selection = select(
        "cli",
        Some(Mode::PathInclude),
        Some(Mode::Pseudonymize),
        None,
    );
    assert!(matches!(selection, Ok(value) if value.mode == Mode::Pseudonymize));
    assert_eq!(
        select(
            "cli",
            Some(Mode::PathInclude),
            Some(Mode::Pseudonymize),
            Some(Mode::PathInclude)
        ),
        Err(Error::OutputExceedsPolicy)
    );
    assert!(
        matches!(select("cli", Some(Mode::PathInclude), None, Some(Mode::AggregateOnly)), Ok(value) if value.mode == Mode::PathInclude)
    );
}

#[test]
fn unknown_channel_is_rejected_and_recipient_never_claims_onward_control() {
    assert_eq!(
        select("unknown", None, None, None),
        Err(Error::UnknownChannel)
    );
    assert!(
        matches!(select("mcp", None, None, None), Ok(value) if value.channel.recipient == Recipient::LocalClientWithUnknownOnward)
    );
    assert!(
        matches!(select("cli", None, None, None), Ok(value) if value.channel.recipient == Recipient::TerminalWithUnknownOnward)
    );
}
