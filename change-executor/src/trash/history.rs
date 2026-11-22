//! Per-item history projection and terminal job summary.

use std::sync::Arc;

use du_change_planner::ports::PlanItem;
use du_core_contracts::{
    SizeAmount,
    change_ipc::{ChangeItemResult, ChangeItemState},
    history_ipc::HistoryItemOutcome,
    job::{ItemHistoryError, ItemHistoryRecord, JobContext, JobCounts, JobOutcome, ObservedSize},
};

use super::results::{ChangeResultHandle, item_result};

pub(super) fn record_remaining(
    remaining: &[PlanItem],
    results: &ChangeResultHandle,
    context: &Arc<dyn JobContext>,
    sequence: &mut u64,
    reason: Option<&'static str>,
) -> bool {
    let mut failed = false;
    for item in remaining {
        let result = item_result(
            item,
            ChangeItemState::Unprocessed,
            reason,
            None,
            false,
            false,
            false,
        );
        results.update(item.item_no, result.clone());
        if history_record(item, &result, *sequence)
            .and_then(|record| context.record_item(record))
            .is_err()
        {
            failed = true;
        }
        *sequence = sequence.saturating_add(1);
    }
    failed
}

pub(super) fn is_path_ancestor(parent: &[Vec<u8>], child: &[Vec<u8>]) -> bool {
    parent.len() < child.len() && child.starts_with(parent)
}

pub(super) fn history_record(
    item: &PlanItem,
    result: &ChangeItemResult,
    item_seq: u64,
) -> Result<ItemHistoryRecord, ItemHistoryError> {
    let location_raw = item
        .path
        .raw
        .bytes()
        .map_err(|_| ItemHistoryError::Invalid)?;
    let outcome = match result.state {
        ChangeItemState::Completed => HistoryItemOutcome::Completed,
        ChangeItemState::Skipped => HistoryItemOutcome::Skipped,
        ChangeItemState::Failed => HistoryItemOutcome::Failed,
        ChangeItemState::Unprocessed => HistoryItemOutcome::Unprocessed,
    };
    let observed_size = result
        .observed_size
        .as_ref()
        .map(|size| ObservedSize::Exact(size.get()))
        .unwrap_or_else(|| expected_observed_size(item));
    Ok(ItemHistoryRecord {
        item_seq,
        item_ref: format!(
            "{}:{}",
            item.item.result_version.as_str(),
            item.item.item_no
        )
        .into_bytes(),
        location_raw,
        location_text: item.path.display.clone(),
        outcome,
        reason_key: result.reason_key.clone(),
        review_needed: result.review_needed,
        held: result.held,
        outcome_unknown: result.outcome_unknown,
        observed_size,
        // History's displaced marker is reserved for overwritten and failed results.
        // The session undo registry owns trash location details for this operation.
        displaced: None,
    })
}

fn expected_observed_size(item: &PlanItem) -> ObservedSize {
    match item.expected_size.as_ref().map(|value| &value.amount) {
        Some(SizeAmount::Exact { bytes }) => ObservedSize::Exact(bytes.get()),
        Some(SizeAmount::LowerBound { bytes }) => ObservedSize::LowerBound(bytes.get()),
        Some(SizeAmount::Unknown) | None => ObservedSize::Unknown,
    }
}

pub(super) fn outcome(
    counts: du_core_contracts::change_ipc::ChangeItemCounts,
    completed_scope: bool,
    failure_reason_key: Option<&str>,
) -> JobOutcome {
    JobOutcome {
        counts: JobCounts {
            completed: u64::from(counts.completed),
            failed: u64::from(counts.failed),
            skipped: u64::from(counts.skipped),
            unprocessed: u64::from(counts.unprocessed),
            overwritten: 0,
        },
        completed_scope,
        failure_reason_key: failure_reason_key.map(str::to_owned),
        unprocessed_summary: (counts.unprocessed > 0).then(|| "change.items_unprocessed".into()),
        result_version_id: None,
    }
}

pub(super) fn now_timestamp() -> Option<du_core_contracts::Timestamp> {
    let millis = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()?;
    du_core_contracts::Timestamp::from_unix_millis(millis)
}
