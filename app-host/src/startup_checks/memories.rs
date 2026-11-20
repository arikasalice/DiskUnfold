//! The remembered startup states of the session (#195).
use du_core_contracts::startup_ipc::StartupMemoryKind;

use super::StartupAccessSession;

/// The remembered state keys of the startup notice and the Support information.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StartupMemories {
    pub(crate) notice_dismissed: Option<String>,
    pub(crate) review_seen: Option<String>,
}

impl StartupAccessSession {
    /// The remembered states; a poisoned lock remembers nothing, so the notice shows.
    pub(crate) fn memories(&self) -> StartupMemories {
        self.memories
            .read()
            .map_or_else(|_| StartupMemories::default(), |memories| memories.clone())
    }

    /// Replaces every remembered state, as loaded from the settings at startup.
    pub(crate) fn load_memories(&self, loaded: StartupMemories) {
        match self.memories.write() {
            Ok(mut memories) => *memories = loaded,
            Err(error) => *error.into_inner() = loaded,
        }
    }

    /// Records one state as stored after a settings commit.
    pub(crate) fn remember(&self, kind: StartupMemoryKind, stored: Option<String>) {
        let mut memories = match self.memories.write() {
            Ok(memories) => memories,
            Err(error) => error.into_inner(),
        };
        match kind {
            StartupMemoryKind::NoticeDismissed => memories.notice_dismissed = stored,
            StartupMemoryKind::ReviewSeen => memories.review_seen = stored,
        }
    }
}
