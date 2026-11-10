//! F37 channel and effective-mode selection; this does not authorize release.
//!
//! Filtering, policy ownership, preview binding, secret checks and output are
//! separate responsibilities. This module only selects a more restrictive mode.

/// Sharing modes ordered from least to most revealing.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Mode {
    AggregateOnly,
    Pseudonymize,
    PathInclude,
}

/// The intended recipient category, not a guarantee about onward transmission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recipient {
    /// A selected local client; it may forward data, including to external AI.
    LocalClientWithUnknownOnward,
    TerminalWithUnknownOnward,
    ReportFile,
    DataFile,
    DiagnosticsFile,
    SettingsFile,
}

/// Immutable channel registration used by the policy selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Channel {
    pub id: &'static str,
    pub recipient: Recipient,
    allowed: &'static [Mode],
}

impl Channel {
    /// Returns whether the channel allows the supplied mode.
    #[must_use]
    pub fn allows(self, mode: Mode) -> bool {
        self.allowed.contains(&mode)
    }
}

const ALL: &[Mode] = &[Mode::AggregateOnly, Mode::Pseudonymize, Mode::PathInclude];
const DIAGNOSTICS: &[Mode] = &[Mode::AggregateOnly, Mode::Pseudonymize];
const SETTINGS: &[Mode] = &[Mode::PathInclude];
const CHANNELS: &[Channel] = &[
    Channel {
        id: "mcp",
        recipient: Recipient::LocalClientWithUnknownOnward,
        allowed: ALL,
    },
    Channel {
        id: "cli",
        recipient: Recipient::TerminalWithUnknownOnward,
        allowed: ALL,
    },
    Channel {
        id: "report",
        recipient: Recipient::ReportFile,
        allowed: ALL,
    },
    Channel {
        id: "data-export",
        recipient: Recipient::DataFile,
        allowed: ALL,
    },
    Channel {
        id: "diagnostics",
        recipient: Recipient::DiagnosticsFile,
        allowed: DIAGNOSTICS,
    },
    Channel {
        id: "settings",
        recipient: Recipient::SettingsFile,
        allowed: SETTINGS,
    },
];

/// A mode-selection failure without candidate contents or paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    UnknownChannel,
    ModeNotAllowed,
    OutputExceedsPolicy,
}

/// Selected mode before strategy lookup, filtering, or release authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selection {
    pub channel: Channel,
    pub mode: Mode,
    /// True when no channel policy was provided and aggregate-only was chosen.
    pub defaulted: bool,
}

/// Selects the channel mode, intersects the session policy, then checks options.
///
/// `channel_mode` must already come from the correct channel policy source;
/// this function cannot identify a connection key or confirm a preview.
/// `session_limit` is an independently established policy restriction.
/// `output_option` is checked against the effective mode. Any narrower output
/// shape is applied by the caller after filtering under that mode.
/// No state is retained between calls.
///
/// # Errors
/// Rejects unknown channels, modes disallowed by that channel, and options
/// requesting information broader than the effective policy.
pub fn select(
    channel_id: &str,
    channel_mode: Option<Mode>,
    session_limit: Option<Mode>,
    output_option: Option<Mode>,
) -> Result<Selection, Error> {
    let channel = CHANNELS
        .iter()
        .copied()
        .find(|row| row.id == channel_id)
        .ok_or(Error::UnknownChannel)?;
    let defaulted = channel_mode.is_none();
    let mode = channel_mode.unwrap_or(Mode::AggregateOnly);
    if !channel.allows(mode) {
        return Err(Error::ModeNotAllowed);
    }
    let mode = session_limit.map_or(mode, |limit| mode.min(limit));
    if !channel.allows(mode) {
        return Err(Error::ModeNotAllowed);
    }
    if let Some(option) = output_option {
        if !channel.allows(option) {
            return Err(Error::ModeNotAllowed);
        }
        if option > mode {
            return Err(Error::OutputExceedsPolicy);
        }
    }
    Ok(Selection {
        channel,
        mode,
        defaulted,
    })
}
