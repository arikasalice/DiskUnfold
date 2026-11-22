//! Current-session results for one F11 executor job.

use std::sync::{Arc, Mutex, MutexGuard};

use du_change_planner::ports::PlanItem;
use du_core_contracts::{
    ByteSize,
    change_ipc::{ChangeItemCounts, ChangeItemResult, ChangeItemState},
    history_ipc::HistoryItemRecording,
    platform_os_change::TrashedUndoInfo,
};

/// One item row retained for the current application session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeResultItem {
    /// Stable position in the immutable plan.
    pub item_no: u32,
    /// Current-session result shown by `change.item_results`.
    pub result: ChangeItemResult,
}

/// Current-session results from one approved change job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeResultSnapshot {
    pub items: Vec<ChangeResultItem>,
    pub counts: ChangeItemCounts,
    pub item_recording: HistoryItemRecording,
}

/// A read handle shared by the job body and the GUI result-query use case.
#[derive(Clone, Default)]
pub struct ChangeResultHandle(Arc<Mutex<ResultState>>);

#[derive(Default)]
struct ResultState {
    items: Vec<ChangeResultItem>,
    item_recording: HistoryItemRecordingState,
    frozen: bool,
}

#[derive(Default)]
struct HistoryItemRecordingState(bool);

impl ChangeResultHandle {
    /// Returns a consistent copy of the in-memory result rows.
    pub fn snapshot(&self) -> ChangeResultSnapshot {
        let state = self.lock();
        let mut counts = ChangeItemCounts::default();
        for item in &state.items {
            match item.result.state {
                ChangeItemState::Completed => counts.completed = counts.completed.saturating_add(1),
                ChangeItemState::Skipped => counts.skipped = counts.skipped.saturating_add(1),
                ChangeItemState::Failed => counts.failed = counts.failed.saturating_add(1),
                ChangeItemState::Unprocessed => {
                    counts.unprocessed = counts.unprocessed.saturating_add(1)
                }
            }
        }
        ChangeResultSnapshot {
            items: state.items.clone(),
            counts,
            item_recording: if state.item_recording.0 {
                HistoryItemRecording::Recorded
            } else {
                HistoryItemRecording::NotRecorded
            },
        }
    }

    pub(super) fn for_items(items: &[PlanItem]) -> Self {
        Self(Arc::new(Mutex::new(ResultState {
            items: items
                .iter()
                .map(|item| ChangeResultItem {
                    item_no: item.item_no,
                    result: item_result(
                        item,
                        ChangeItemState::Unprocessed,
                        None,
                        None,
                        false,
                        false,
                        false,
                    ),
                })
                .collect(),
            item_recording: HistoryItemRecordingState(false),
            frozen: false,
        })))
    }

    pub(super) fn begin_history(&self) {
        self.lock().item_recording.0 = true;
    }

    pub(super) fn update(&self, item_no: u32, result: ChangeItemResult) {
        let mut state = self.lock();
        if state.frozen {
            return;
        }
        if let Some(item) = state.items.iter_mut().find(|item| item.item_no == item_no) {
            item.result = result;
        }
    }

    pub(super) fn freeze(&self) {
        self.lock().frozen = true;
    }

    pub(super) fn set_undo_available(&self, item_no: u32, available: bool) {
        let mut state = self.lock();
        if !state.frozen
            && let Some(item) = state.items.iter_mut().find(|item| item.item_no == item_no)
        {
            item.result.undo_available = available;
        }
    }

    fn lock(&self) -> MutexGuard<'_, ResultState> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub(super) fn item_result(
    item: &PlanItem,
    state: ChangeItemState,
    reason: Option<&str>,
    observed_size: Option<ByteSize>,
    review_needed: bool,
    held: bool,
    outcome_unknown: bool,
) -> ChangeItemResult {
    ChangeItemResult {
        item_ref: item.item.clone(),
        state,
        reason_key: reason.map(str::to_owned),
        observed_size,
        review_needed,
        held,
        outcome_unknown,
        undo_available: false,
    }
}

pub(super) fn simple(
    item: &PlanItem,
    state: ChangeItemState,
    reason: &'static str,
) -> (ChangeItemResult, Option<TrashedUndoInfo>, bool) {
    (
        item_result(item, state, Some(reason), None, false, false, false),
        None,
        false,
    )
}

pub(super) fn with_size(
    item: &PlanItem,
    state: ChangeItemState,
    reason: &'static str,
    size: Option<ByteSize>,
) -> (ChangeItemResult, Option<TrashedUndoInfo>, bool) {
    (
        item_result(item, state, Some(reason), size, false, false, false),
        None,
        false,
    )
}
