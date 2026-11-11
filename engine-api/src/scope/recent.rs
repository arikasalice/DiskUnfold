//! Apply the F01 recent-scope correspondence rules to platform evidence.
use std::collections::BTreeMap;

use du_core_contracts::{
    OsPath,
    analysis_ipc::{ScopeDraft, ScopeReview, ScopeSource},
    ports::{PortError, PortErrorKind},
    scan_observation::{OpenedScopeRoot, ScopeRootPort},
    volume::RecentVolumePort,
};
use du_job_runtime::JobClock;
use du_scan_engine::scope::recent::{
    Correspondence, CurrentVolume, EntryComparison, Identity, Inventory, correspond,
};

/// Correspondence of a saved volume at review time.
#[derive(Debug, PartialEq, Eq)]
pub enum RecentVolumeMatch {
    Matched,
    /// A proposal for user review. It does not change the restored scope.
    Moved {
        proposed_entry: OsPath,
    },
    Disconnected,
    Unverifiable,
}

/// Rechecks a saved volume using only its owning adapter's confirmed evidence.
pub fn review_recent_volume(
    port: &dyn RecentVolumePort,
    saved_id: &[u8],
    saved_entry: &OsPath,
    continue_work: &dyn Fn() -> bool,
) -> RecentVolumeMatch {
    let Some(observation) = port.recent_volume(saved_id, saved_entry, continue_work) else {
        return RecentVolumeMatch::Unverifiable;
    };
    let identity = Identity::Confirmed {
        kind: observation.identity_kind,
        value: saved_id,
    };
    let paths = observation
        .entries
        .iter()
        .map(|entry| entry.path.raw.bytes())
        .collect::<Result<Vec<_>, _>>();
    let Ok(paths) = paths else {
        return RecentVolumeMatch::Unverifiable;
    };
    let entries = paths
        .iter()
        .zip(&observation.entries)
        .map(|(raw, entry)| CurrentVolume {
            identity,
            entry_path: raw,
            entry_comparison: if entry.same_entry {
                EntryComparison::Same
            } else {
                EntryComparison::Different
            },
        })
        .collect::<Vec<_>>();
    match correspond(identity, Inventory::Complete, &entries) {
        Correspondence::Matched => RecentVolumeMatch::Matched,
        Correspondence::Moved { proposed_entry } => paths
            .iter()
            .position(|raw| raw == proposed_entry)
            .map(|index| RecentVolumeMatch::Moved {
                proposed_entry: observation.entries[index].path.clone(),
            })
            .unwrap_or(RecentVolumeMatch::Unverifiable),
        Correspondence::Disconnected => RecentVolumeMatch::Disconnected,
        Correspondence::Unverifiable => RecentVolumeMatch::Unverifiable,
    }
}

/// Reuses scope normalization after binding every actual open to its saved volume.
/// The caller must first confirm correspondence for every saved entry.
pub fn review_matched_scope(
    draft: &ScopeDraft,
    request_seq: u32,
    volumes: &[Vec<u8>],
    roots: &dyn ScopeRootPort,
    now_ms: fn() -> i64,
    continue_work: &(dyn Fn() -> bool + Sync),
) -> Option<ScopeReview> {
    if draft.source != ScopeSource::Recent || draft.includes.len() != volumes.len() {
        return None;
    }
    let mut effective = draft.clone();
    effective.source = ScopeSource::Gui;
    super::validate(&effective, false).ok()?;
    let mut expected = BTreeMap::new();
    for (path, volume) in effective.includes.iter().zip(volumes) {
        if volume.is_empty()
            || expected
                .insert(path.raw.bytes().ok()?, volume.as_slice())
                .is_some_and(|old| old != volume)
        {
            return None;
        }
    }
    let bound = BoundRecentRoots {
        roots,
        expected,
        continue_work,
    };
    let mut review =
        super::review_multiple(effective, request_seq, &bound, &ReviewClock(now_ms)).ok()?;
    review.scope.source = ScopeSource::Recent;
    continue_work().then_some(review)
}

struct BoundRecentRoots<'a> {
    roots: &'a dyn ScopeRootPort,
    expected: BTreeMap<Vec<u8>, &'a [u8]>,
    continue_work: &'a (dyn Fn() -> bool + Sync),
}

impl ScopeRootPort for BoundRecentRoots<'_> {
    fn open_root(&self, path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let changed = || PortError {
            kind: PortErrorKind::IdentityChanged,
            reason_key: "scope.changed".into(),
            os_code: None,
        };
        let expected = self.expected.get(path).ok_or_else(changed)?;
        if !(self.continue_work)() {
            return Err(changed());
        }
        let opened = self.roots.open_root(path)?;
        if &opened.volume_id.as_slice() != expected {
            return Err(changed());
        }
        Ok(opened)
    }
}

struct ReviewClock(fn() -> i64);

impl JobClock for ReviewClock {
    fn now_ms(&self) -> i64 {
        (self.0)()
    }
}

#[cfg(test)]
mod tests;
