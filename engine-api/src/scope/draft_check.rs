//! Shape checks of a scope draft before any root is opened.
use super::exclusion;
use crate::{api::Channel, error::EngineError};
use du_core_contracts::analysis_ipc::{ScopeBoundary, ScopeDraft, ScopeSource};

/// The draft source a channel submits: the command line names its scopes `cli`,
/// the screen `gui` (#222).
pub(super) fn source_of(channel: Channel) -> ScopeSource {
    match channel {
        Channel::Cli => ScopeSource::Cli,
        Channel::Gui | Channel::Mcp => ScopeSource::Gui,
    }
}

pub(super) fn validate(
    draft: &ScopeDraft,
    has_comparison: bool,
    channel: Channel,
) -> Result<(), EngineError> {
    if !draft.exclusions.is_empty() {
        exclusion::check(draft, has_comparison)?;
    }
    if draft.includes.is_empty() {
        return Err(EngineError::reason("scope.empty"));
    }
    if draft.source != source_of(channel)
        || draft.boundary != ScopeBoundary::CurrentVolume
        || !draft.included_mounts.is_empty()
    {
        return Err(EngineError::reason("scope.target_unavailable"));
    }
    Ok(())
}
