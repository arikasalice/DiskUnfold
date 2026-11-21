//! Execute one approved F11 `change.trash` job.
//!
//! Design items: F11 FR2.HD1.app-host.DD1, FR3.HD1.app-host.DD1,
//! FR6.HD1.app-host.DD1, FR7.HD1.app-host.DD1, FR8.HD1.app-host.DD1,
//! FR11.HD1.app-host.DD1, FR14.HD1.app-host.DD2, F21 FR3.HD1.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use du_change_planner::ports::{
    PlanAction, PlanItem, PlanItemState, PlanRejudgement, PlanVersion, RejudgementState,
};
use du_core_contracts::{
    Id,
    change_ipc::{
        CHANGE_ITEM_ALREADY_PROCESSED, CHANGE_ITEM_CANCELLED, CHANGE_ITEM_CHANGED,
        CHANGE_ITEM_IDENTITY_CHANGED, CHANGE_ITEM_IDENTITY_RACE, CHANGE_ITEM_IDENTITY_UNSUPPORTED,
        CHANGE_ITEM_OS_FAILED, CHANGE_ITEM_PLAN_CONFLICT, CHANGE_ITEM_TRASH_UNSUPPORTED,
        CHANGE_ITEM_VOLUME_UNAVAILABLE, ChangeItemResult, ChangeItemState,
    },
    job::{
        Checkpoint, HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobOutcome,
        JobProgress, ProgressUnit, ResourceClass,
    },
    platform_os_change::{
        CHANGE_TRASH, CHANGE_TRASH_IDENTITY_BOUND, ChangeOpsPort, SessionUndoStore, TrashedUndoInfo,
    },
    ports::EntryKind,
    result::ResultItemKind,
    volume::CapabilityState,
};

mod decisions;
mod history;
mod results;
use decisions::{lookup_error, metadata_matches, resolve_move};
use history::{history_record, is_path_ancestor, now_timestamp, outcome, record_remaining};
pub use results::{ChangeResultHandle, ChangeResultItem, ChangeResultSnapshot};
use results::{item_result, simple, with_size};

/// Descriptor for the cancellable, serialized F11 trash operation.
pub fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: CHANGE_TRASH.into(),
        version: 1,
        display_name_key: "job.change_trash".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["change.item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::FileChange],
        history: HistoryMode::PerItem,
    }
}

/// One item row retained for the current application session.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProcessedState {
    InFlight,
    Finished,
}

/// Process-local idempotency markers for one immutable request version.
struct ProcessedRecord {
    request_version_id: Id,
    plan_hash: [u8; 32],
    items: BTreeMap<u32, ProcessedState>,
    completed: BTreeSet<u32>,
}

/// F11 identity-bound trash job body. It never performs filesystem I/O itself.
pub struct ChangeTrashJob {
    plan: PlanVersion,
    items: Vec<PlanItem>,
    protection: BTreeMap<u32, RejudgementState>,
    ops: Arc<dyn ChangeOpsPort>,
    undo: Arc<dyn SessionUndoStore>,
    job_ref: Mutex<Option<Id>>,
    processed: Mutex<ProcessedRecord>,
    results: ChangeResultHandle,
}

impl ChangeTrashJob {
    /// Creates the one-per-item job from a freshly verified plan and re-judgement.
    pub fn new(
        plan: PlanVersion,
        rejudgement: PlanRejudgement,
        ops: Arc<dyn ChangeOpsPort>,
        undo: Arc<dyn SessionUndoStore>,
    ) -> Result<Self, &'static str> {
        if rejudgement.version_id != plan.version_id || rejudgement.recomputed_hash != plan.hash {
            return Err("approval.mismatch");
        }
        let mut items = plan
            .items
            .iter()
            .filter(|item| {
                item.state == PlanItemState::Included && item.action == PlanAction::Trash
            })
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.path
                .raw
                .bytes()
                .unwrap_or_default()
                .cmp(&right.path.raw.bytes().unwrap_or_default())
                .then_with(|| left.item_no.cmp(&right.item_no))
        });
        if items.is_empty() {
            return Err("change.no_trash_items");
        }
        let protection = rejudgement.items.into_iter().collect::<BTreeMap<_, _>>();
        let results = ChangeResultHandle::for_items(&items);
        Ok(Self {
            processed: Mutex::new(ProcessedRecord {
                request_version_id: plan.version_id.clone(),
                plan_hash: plan.hash,
                items: BTreeMap::new(),
                completed: BTreeSet::new(),
            }),
            plan,
            items,
            protection,
            ops,
            undo,
            job_ref: Mutex::new(None),
            results,
        })
    }

    /// Returns the session result handle associated during job admission.
    pub fn results(&self) -> ChangeResultHandle {
        self.results.clone()
    }

    /// Binds the runtime-assigned job reference before the job becomes visible to workers.
    pub fn bind_job_ref(&self, job_ref: Id) -> Result<(), &'static str> {
        let mut slot = self.job_ref.lock().map_err(|_| "support.internal_error")?;
        if slot.is_some() {
            return Err("job.invalid_state");
        }
        *slot = Some(job_ref);
        Ok(())
    }
}

impl JobBody for ChangeTrashJob {
    fn kind_id(&self) -> &str {
        "change.trash"
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        self.results.begin_history();
        let Some(job_ref) = self.job_ref.lock().ok().and_then(|value| value.clone()) else {
            self.results.freeze();
            return outcome(
                self.results.snapshot().counts,
                false,
                Some("support.internal_error"),
            );
        };
        let mut sequence = 0_u64;
        let mut unavailable_volumes = BTreeSet::new();
        let mut processed_any = false;

        for (index, item) in self.items.iter().enumerate() {
            if context.checkpoint() == Checkpoint::Cancel {
                let failed = record_remaining(
                    &self.items[index..],
                    &self.results,
                    &context,
                    &mut sequence,
                    Some(CHANGE_ITEM_CANCELLED),
                );
                self.results.freeze();
                return outcome(
                    self.results.snapshot().counts,
                    false,
                    failed.then_some("history.record_failed"),
                );
            }

            let result = if self.was_processed(item.item_no) {
                simple(
                    item,
                    ChangeItemState::Skipped,
                    CHANGE_ITEM_ALREADY_PROCESSED,
                )
            } else if unavailable_volumes.contains(&item.volume_id) {
                simple(
                    item,
                    ChangeItemState::Skipped,
                    CHANGE_ITEM_VOLUME_UNAVAILABLE,
                )
            } else {
                match self.protection.get(&item.item_no).copied() {
                    Some(RejudgementState::Same) => {
                        if self.has_prior_conflict(index, item) {
                            simple(
                                item,
                                ChangeItemState::Unprocessed,
                                CHANGE_ITEM_PLAN_CONFLICT,
                            )
                        } else {
                            self.process_item(item, &mut unavailable_volumes)
                        }
                    }
                    Some(state) => {
                        let reason = match state {
                            RejudgementState::Denied => "protect.denied",
                            RejudgementState::Undetermined => "protect.undetermined",
                            RejudgementState::ClassChanged => "protect.changed",
                            RejudgementState::Same => "protect.changed",
                        };
                        (
                            item_result(
                                item,
                                ChangeItemState::Skipped,
                                Some(reason),
                                None,
                                true,
                                state == RejudgementState::Denied,
                                false,
                            ),
                            None,
                            false,
                        )
                    }
                    None => (
                        item_result(
                            item,
                            ChangeItemState::Skipped,
                            Some("protect.undetermined"),
                            None,
                            true,
                            true,
                            false,
                        ),
                        None,
                        false,
                    ),
                }
            };

            let (result, undo_info, stop) = result;
            self.results.update(item.item_no, result.clone());
            if let Some(info) = undo_info {
                self.register_undo(&job_ref, vec![info]);
            }
            self.mark_processed(
                item.item_no,
                ProcessedState::Finished,
                result.state == ChangeItemState::Completed,
            );
            processed_any = true;
            if history_record(item, &result, sequence)
                .and_then(|record| context.record_item(record))
                .is_err()
            {
                let remaining = &self.items[index + 1..];
                for pending in remaining {
                    self.results.update(
                        pending.item_no,
                        item_result(
                            pending,
                            ChangeItemState::Unprocessed,
                            None,
                            None,
                            false,
                            false,
                            false,
                        ),
                    );
                }
                self.results.freeze();
                return outcome(
                    self.results.snapshot().counts,
                    false,
                    Some("history.record_failed"),
                );
            }
            sequence = sequence.saturating_add(1);
            context.report_progress(JobProgress {
                completed: (index + 1) as u64,
                total: Some(self.items.len() as u64),
            });

            if stop {
                let failed = record_remaining(
                    &self.items[index + 1..],
                    &self.results,
                    &context,
                    &mut sequence,
                    None,
                );
                self.results.freeze();
                return outcome(
                    self.results.snapshot().counts,
                    false,
                    failed.then_some("history.record_failed"),
                );
            }
        }

        let snapshot = self.results.snapshot();
        let complete = processed_any
            && snapshot.counts.skipped == 0
            && snapshot.counts.failed == 0
            && snapshot.counts.unprocessed == 0;
        self.results.freeze();
        outcome(snapshot.counts, complete, None)
    }
}

impl ChangeTrashJob {
    fn was_processed(&self, item_no: u32) -> bool {
        let state = self
            .processed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.request_version_id == self.plan.version_id
            && state.plan_hash == self.plan.hash
            && state.items.contains_key(&item_no)
    }

    fn mark_processed(&self, item_no: u32, state: ProcessedState, completed: bool) {
        let mut processed = self
            .processed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        processed.items.insert(item_no, state);
        if state == ProcessedState::Finished && completed {
            processed.completed.insert(item_no);
        }
    }

    fn has_prior_conflict(&self, index: usize, item: &PlanItem) -> bool {
        let completed = self
            .processed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .completed
            .clone();
        self.items[..index].iter().any(|prior| {
            completed.contains(&prior.item_no)
                && is_path_ancestor(&prior.path_components, &item.path_components)
        })
    }

    fn process_item(
        &self,
        item: &PlanItem,
        unavailable_volumes: &mut BTreeSet<Vec<u8>>,
    ) -> (ChangeItemResult, Option<TrashedUndoInfo>, bool) {
        let Some(expected) = item.expected_identity.as_ref() else {
            return simple(
                item,
                ChangeItemState::Skipped,
                CHANGE_ITEM_IDENTITY_UNSUPPORTED,
            );
        };
        let kind = match item.expected_kind {
            ResultItemKind::File => EntryKind::File,
            ResultItemKind::Directory => {
                // The current port has no child-enumeration and replacement-race guarantee.
                return simple(
                    item,
                    ChangeItemState::Skipped,
                    CHANGE_ITEM_TRASH_UNSUPPORTED,
                );
            }
            ResultItemKind::UntraversedLink
            | ResultItemKind::Unknown
            | ResultItemKind::UnknownStructure => {
                return simple(
                    item,
                    ChangeItemState::Skipped,
                    CHANGE_ITEM_IDENTITY_UNSUPPORTED,
                );
            }
        };
        let observed = match self.ops.lookup_no_follow(&item.path) {
            Ok(observed) => observed,
            Err(error) => return lookup_error(item, error, unavailable_volumes),
        };
        if observed.identity.volume_id != item.volume_id {
            return simple(item, ChangeItemState::Skipped, CHANGE_ITEM_CHANGED);
        }
        if observed.identity.volume_id != expected.volume_id
            || observed.identity.file_id != expected.file_id
            || observed.kind != kind
        {
            return simple(item, ChangeItemState::Skipped, CHANGE_ITEM_IDENTITY_CHANGED);
        }
        if !metadata_matches(item, &observed) {
            return with_size(
                item,
                ChangeItemState::Skipped,
                CHANGE_ITEM_CHANGED,
                observed.size,
            );
        }
        let capability = self
            .ops
            .capability(&item.volume_id, CHANGE_TRASH_IDENTITY_BOUND);
        if capability.state != CapabilityState::Supported {
            let reason = if capability.reason_key == Some("change.identity_unsupported") {
                CHANGE_ITEM_IDENTITY_UNSUPPORTED
            } else {
                CHANGE_ITEM_TRASH_UNSUPPORTED
            };
            return simple(item, ChangeItemState::Skipped, reason);
        }
        self.mark_processed(item.item_no, ProcessedState::InFlight, false);

        let moved_at = now_timestamp();
        let move_outcome = self
            .ops
            .trash_identity_bound(&item.path, &observed.identity);
        let (state, reason, outcome_unknown, actual_identity, location, stop, failure_volume) =
            resolve_move(&*self.ops, &item.path, &observed.identity, move_outcome);
        if failure_volume {
            unavailable_volumes.insert(item.volume_id.clone());
        }
        let result = item_result(
            item,
            state,
            reason,
            observed.size.clone(),
            state == ChangeItemState::Failed && reason == Some(CHANGE_ITEM_IDENTITY_RACE),
            false,
            outcome_unknown,
        );
        let undo = if matches!(state, ChangeItemState::Completed | ChangeItemState::Failed)
            && reason != Some(CHANGE_ITEM_OS_FAILED)
        {
            actual_identity
                .zip(moved_at)
                .map(|(identity, moved_at)| TrashedUndoInfo {
                    item_no: item.item_no,
                    item_ref: item.item.clone(),
                    original_path: item.path.clone(),
                    identity,
                    kind,
                    size: observed.size.clone(),
                    modified_at: observed.modified_at,
                    children_fingerprint: None,
                    parent_identity: None,
                    trash_location: location,
                    moved_at,
                })
        } else {
            None
        };
        (result, undo, stop)
    }

    fn register_undo(&self, job_ref: &Id, items: Vec<TrashedUndoInfo>) {
        let items = items
            .into_iter()
            .filter(|item| item.trash_location.is_some())
            .collect::<Vec<_>>();
        if items.is_empty() || self.undo.register_trashed(job_ref, &items).is_err() {
            return;
        }
        for item in items {
            if self.undo.contains(job_ref, item.item_no).unwrap_or(false) {
                self.results.set_undo_available(item.item_no, true);
            }
        }
    }
}
