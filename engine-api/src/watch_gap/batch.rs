//! Adapter for the validated watch consumer callback. No jobs are submitted here.
//!
//! The caller must preserve and commit the original Update, including every gap,
//! replacement, timestamp and resync boundary, atomically with this result.
//! This plan does not carry freshness evidence or implement IF-17 idempotency.
//! The scope owner resolves every path against the pinned scope, exclusions, access,
//! mount boundaries and volume identity. Automatic refresh needs verified evidence.

pub use super::recovery::Volume;
use super::{
    Update,
    recovery::{self, BaseResult, Decision, GapScope, RangeEvidence, RecoveryReason},
};
use crate::refresh_scope::{self, CheckedDirectory, TargetSelection};
use du_scan_engine::watch_coalescer::Key;
use std::collections::{BTreeMap, BTreeSet};

/// Evidence produced by the scope owner, never from an IPC caller's flags.
pub struct Admitted<T, C> {
    pub target: T,
    pub components: Vec<C>,
    pub base: BaseResult,
    /// Only a positive, verified match admits automatic work.
    pub volume: Volume,
}

/// Implemented by the trusted owner of the pinned scope and its evidence.
pub trait ScopeOwner {
    type Target: Ord;
    type Component: Ord;
    /// For a path, `None` means outside scope, inaccessible, or an unknown boundary.
    /// `path == None` asks for the retained included target binding. That binding
    /// can remain valid when the volume's current availability is unknown.
    fn admit(
        &mut self,
        scope: [u8; 16],
        watch: [u8; 16],
        path: Option<&Key>,
    ) -> Option<Admitted<Self::Target, Self::Component>>;
}

#[derive(Debug, PartialEq, Eq)]
pub struct Directory<C> {
    pub components: Vec<C>,
    pub include_descendants: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Selection<C> {
    Directories(Vec<Directory<C>>),
    WholeTarget,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Plan<T, C> {
    pub scope: [u8; 16],
    pub automatic: BTreeMap<T, Selection<C>>,
    pub recovery: Vec<(T, RecoveryReason)>,
}

/// Convert exactly one consumer update. Results from different scope revisions
/// must remain separate. The limit is applied after minimization per target.
pub fn prepare<O: ScopeOwner>(
    scope: [u8; 16],
    update: Update<'_>,
    owner: &mut O,
    limit: usize,
) -> Option<Plan<O::Target, O::Component>> {
    let mut automatic = Vec::new();
    let mut recovery = Vec::new();
    let mut automatic_gap_ids = BTreeSet::new();
    let (watch, gaps) = match update {
        Update::Batch {
            batch,
            previous_gaps,
            gaps,
        } => {
            if batch.context.scope_revision != scope {
                return None;
            }
            for change in &batch.changes {
                let evidence = RangeEvidence::Change {
                    include_descendants: change.subtree,
                };
                add(
                    owner.admit(scope, batch.context.watch_ref, Some(&change.path)),
                    evidence,
                    false,
                    &mut automatic,
                    &mut recovery,
                );
            }
            for gap in &batch.gaps {
                let is_summary = !batch.replaced_gap_ids.is_empty()
                    && !previous_gaps.contains_key(&gap.id)
                    && gap.end.is_some();
                let is_new = previous_gaps
                    .get(&gap.id)
                    .is_none_or(|old| old.end.is_none() && gap.end.is_some());
                if !is_summary && is_new {
                    automatic_gap_ids.insert(gap.id);
                }
            }
            (batch.context.watch_ref, Some(gaps))
        }
        Update::Snapshot {
            snapshot,
            gaps,
            resync_required,
            ..
        } => {
            if snapshot.definition.scope_revision != scope {
                return None;
            }
            let watch = snapshot.definition.watch_ref;
            if resync_required {
                add(
                    owner.admit(scope, watch, None),
                    RangeEvidence::WholeUnitGap(recovery::WholeUnitGap::Unlocatable),
                    false,
                    &mut automatic,
                    &mut recovery,
                );
            }
            (watch, Some(gaps))
        }
        Update::VolumeReplaced {
            scope: update_scope,
            watch_ref,
            ..
        } => {
            if update_scope != scope {
                return None;
            }
            add(
                owner.admit(scope, watch_ref, None),
                RangeEvidence::WholeUnitGap(recovery::WholeUnitGap::Unlocatable),
                true,
                &mut automatic,
                &mut recovery,
            );
            (watch_ref, None)
        }
    };
    if let Some(gaps) = gaps {
        for gap in gaps.values() {
            classify(
                owner,
                scope,
                watch,
                gap,
                automatic_gap_ids.contains(&gap.id),
                &mut automatic,
                &mut recovery,
            );
        }
    }
    let automatic = refresh_scope::minimize(automatic, limit)
        .into_iter()
        .map(|(target, selection)| {
            let selection = match selection {
                TargetSelection::WholeTarget => Selection::WholeTarget,
                TargetSelection::Directories(rows) => Selection::Directories(
                    rows.into_iter()
                        .map(|d| Directory {
                            components: d.components,
                            include_descendants: d.include_descendants,
                        })
                        .collect(),
                ),
            };
            (target, selection)
        })
        .collect();
    recovery.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.key().cmp(b.1.key())));
    recovery.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    Some(Plan {
        scope,
        automatic,
        recovery,
    })
}

// Full snapshot gaps are for recovery lookup, never automatic readmission.
fn classify<O: ScopeOwner>(
    owner: &mut O,
    scope: [u8; 16],
    watch: [u8; 16],
    gap: &du_scan_engine::watch_coalescer::Gap,
    automatic_allowed: bool,
    automatic: &mut Vec<CheckedDirectory<O::Target, O::Component>>,
    recovery_rows: &mut Vec<(O::Target, RecoveryReason)>,
) {
    let classified = recovery::classify_gap(gap);
    if let Some(evidence) = classified.range_evidence {
        let path = match classified.scope {
            GapScope::Unit => None,
            GapScope::Directory(path) => Some(path),
        };
        if automatic_allowed {
            add(
                owner.admit(scope, watch, path),
                evidence,
                false,
                automatic,
                recovery_rows,
            );
        } else {
            // Retain recovery decisions while discarding any automatic range.
            add(
                owner.admit(scope, watch, path),
                evidence,
                false,
                &mut Vec::new(),
                recovery_rows,
            );
        }
    }
}

fn add<T, C>(
    admitted: Option<Admitted<T, C>>,
    evidence: RangeEvidence,
    replaced: bool,
    automatic: &mut Vec<CheckedDirectory<T, C>>,
    recovery: &mut Vec<(T, RecoveryReason)>,
) {
    let Some(a) = admitted else {
        return;
    };
    let volume = if replaced { Volume::Replaced } else { a.volume };
    match recovery::decide(true, a.base, volume, evidence) {
        Decision::RecoverTarget(reason) => recovery.push((a.target, reason)),
        Decision::QueueRange {
            include_descendants,
        } => automatic.push(CheckedDirectory {
            target: a.target,
            components: a.components,
            include_descendants,
        }),
        Decision::Discard => {}
    }
}

#[cfg(test)]
mod tests;
