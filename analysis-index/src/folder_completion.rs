//! Folder completion proven by the attempt evidence pinned in the same result version.
use du_core_contracts::result::{ResultCompleteness, ResultItemKind};

use crate::{
    folder_evidence_api::FolderAttemptStatus, folder_evidence_snapshot::FolderEvidenceSnapshot,
    tree::BuiltItems,
};

const UNPROVEN: u8 = 0;
const PROVEN: u8 = 1;
const BLOCKED: u8 = 2;

/// Items whose aggregates may use `Complete` as their source instead of the version state.
///
/// A folder is proven when it has at least one pinned attempt, every attempt for it is
/// `EndedUnproven` (enumeration end and direct-child terminals validated in this build), no
/// direct child is an unknown structure, and every subfolder is proven. A non-folder item is
/// proven when its parent folder is. Values still follow the per-basis accumulation rules,
/// and the gap marking pass lowers any gap and its ancestors afterwards.
pub(crate) struct ProvenFolders(Vec<u8>);

impl ProvenFolders {
    /// The completeness an item's aggregate starts from.
    pub(crate) fn source(
        proven: Option<&Self>,
        index: usize,
        version: ResultCompleteness,
    ) -> ResultCompleteness {
        match proven {
            Some(Self(states)) if states.get(index) == Some(&PROVEN) => {
                ResultCompleteness::Complete
            }
            _ => version,
        }
    }

    /// Decides proven items for a sealed `Missing` or `Cancelled` version or a partial
    /// publication. A `Complete` version, a replacement build and a version without pinned
    /// evidence promote nothing.
    pub(crate) fn decide(
        built: &BuiltItems,
        evidence: Option<&FolderEvidenceSnapshot>,
        completeness: ResultCompleteness,
        replacement: bool,
    ) -> Option<Self> {
        if completeness == ResultCompleteness::Complete || replacement {
            return None;
        }
        let summaries = evidence?.summaries();
        let mut states = vec![UNPROVEN; built.items.len()];
        for summary in summaries {
            let state = states.get_mut(summary.folder_item as usize)?;
            *state = match (summary.status, *state) {
                (FolderAttemptStatus::EndedUnproven, UNPROVEN | PROVEN) => PROVEN,
                _ => BLOCKED,
            };
        }
        // Children are numbered after their parent, so a reverse pass sees every child first.
        for item in built.items.iter().rev() {
            let Some(parent) = item.parent else {
                continue;
            };
            let incomplete = match item.kind {
                ResultItemKind::Directory => states[item.item_no as usize] != PROVEN,
                ResultItemKind::UnknownStructure => true,
                _ => false,
            };
            if incomplete {
                states[parent as usize] = BLOCKED;
            }
        }
        let mut any = false;
        for item in &built.items {
            let index = item.item_no as usize;
            if item.kind != ResultItemKind::Directory {
                let parent_proven = item
                    .parent
                    .is_some_and(|parent| states[parent as usize] == PROVEN);
                states[index] = if parent_proven { PROVEN } else { UNPROVEN };
            }
            any |= states[index] == PROVEN;
        }
        any.then_some(Self(states))
    }
}

#[cfg(test)]
mod tests;
