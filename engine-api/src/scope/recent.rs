//! Apply the F01 recent-scope correspondence rules to platform evidence.
use du_core_contracts::{OsPath, volume::RecentVolumePort};
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

#[cfg(test)]
mod tests;
