//! Per-item validation and mapping of operating-system outcomes.

use super::results::{item_result, simple};
use du_change_planner::ports::PlanItem;
use du_core_contracts::{
    OsPath, SizeAmount,
    change_ipc::{
        CHANGE_ITEM_FILE_IN_USE, CHANGE_ITEM_IDENTITY_CHANGED, CHANGE_ITEM_IDENTITY_RACE,
        CHANGE_ITEM_IDENTITY_UNSUPPORTED, CHANGE_ITEM_OS_FAILED, CHANGE_ITEM_OUTCOME_UNKNOWN,
        CHANGE_ITEM_PERMISSION_DENIED, CHANGE_ITEM_TRASH_UNSUPPORTED, ChangeItemResult,
        ChangeItemState,
    },
    platform_os_change::{
        ChangeIdentity, ChangeItemObservation, ChangeOpsPort, TrashLocationRef, TrashMoveOutcome,
        TrashOutcomeQuery, TrashRefusal, TrashedUndoInfo,
    },
    ports::{PortError, PortErrorKind},
    result::ResultItemKind,
};
use std::collections::BTreeSet;

pub(super) fn metadata_matches(item: &PlanItem, observed: &ChangeItemObservation) -> bool {
    let Some(expected_modified_at) = item.expected_modified_at.as_ref() else {
        return false;
    };
    let modified_matches = observed.modified_at.as_ref() == Some(expected_modified_at);
    if item.expected_kind == ResultItemKind::Directory {
        return modified_matches;
    }
    let size_matches = match item.expected_size.as_ref().map(|value| &value.amount) {
        Some(SizeAmount::Exact { bytes }) => observed.size.as_ref() == Some(bytes),
        _ => false,
    };
    size_matches && modified_matches
}

pub(super) fn lookup_error(
    item: &PlanItem,
    error: PortError,
    unavailable_volumes: &mut BTreeSet<Vec<u8>>,
) -> (ChangeItemResult, Option<TrashedUndoInfo>, bool) {
    match error.kind {
        PortErrorKind::PermissionDenied => simple(
            item,
            ChangeItemState::Skipped,
            CHANGE_ITEM_PERMISSION_DENIED,
        ),
        PortErrorKind::InUse => simple(item, ChangeItemState::Skipped, CHANGE_ITEM_FILE_IN_USE),
        PortErrorKind::Unsupported => simple(
            item,
            ChangeItemState::Skipped,
            CHANGE_ITEM_IDENTITY_UNSUPPORTED,
        ),
        PortErrorKind::IdentityChanged | PortErrorKind::NotFound => {
            simple(item, ChangeItemState::Skipped, CHANGE_ITEM_IDENTITY_CHANGED)
        }
        _ if error.reason_key == "scan.disconnected" => {
            unavailable_volumes.insert(item.volume_id.clone());
            (
                item_result(
                    item,
                    ChangeItemState::Failed,
                    Some(CHANGE_ITEM_OS_FAILED),
                    None,
                    false,
                    false,
                    false,
                ),
                None,
                false,
            )
        }
        _ => (
            item_result(
                item,
                ChangeItemState::Failed,
                Some(CHANGE_ITEM_OS_FAILED),
                None,
                false,
                false,
                false,
            ),
            None,
            false,
        ),
    }
}

pub(super) fn resolve_move(
    ops: &dyn ChangeOpsPort,
    path: &OsPath,
    expected: &ChangeIdentity,
    outcome: TrashMoveOutcome,
) -> (
    ChangeItemState,
    Option<&'static str>,
    bool,
    Option<ChangeIdentity>,
    Option<TrashLocationRef>,
    bool,
    bool,
) {
    match outcome {
        TrashMoveOutcome::Moved {
            identity_after_move,
            trash_location,
        } if same_object(&identity_after_move, expected) => (
            ChangeItemState::Completed,
            None,
            false,
            Some(identity_after_move),
            trash_location,
            false,
            false,
        ),
        TrashMoveOutcome::Moved {
            identity_after_move,
            trash_location,
        } => (
            ChangeItemState::Failed,
            Some(CHANGE_ITEM_IDENTITY_RACE),
            false,
            Some(identity_after_move),
            trash_location,
            true,
            false,
        ),
        TrashMoveOutcome::Refused(TrashRefusal::IdentityChanged) => (
            ChangeItemState::Skipped,
            Some(CHANGE_ITEM_IDENTITY_CHANGED),
            false,
            None,
            None,
            false,
            false,
        ),
        TrashMoveOutcome::Refused(TrashRefusal::Unsupported) => (
            ChangeItemState::Skipped,
            Some(CHANGE_ITEM_TRASH_UNSUPPORTED),
            false,
            None,
            None,
            false,
            false,
        ),
        TrashMoveOutcome::OsError(error) => move_error(error),
        TrashMoveOutcome::OutcomeUnknown => match ops.query_trash_outcome(path, expected, None) {
            Ok(TrashOutcomeQuery::Moved {
                identity,
                trash_location,
            }) if same_object(&identity, expected) => (
                ChangeItemState::Completed,
                None,
                false,
                Some(identity),
                trash_location,
                false,
                false,
            ),
            Ok(TrashOutcomeQuery::Moved {
                identity,
                trash_location,
            }) => (
                ChangeItemState::Failed,
                Some(CHANGE_ITEM_IDENTITY_RACE),
                false,
                Some(identity),
                trash_location,
                true,
                false,
            ),
            Ok(TrashOutcomeQuery::NotMoved) => (
                ChangeItemState::Failed,
                Some(CHANGE_ITEM_OS_FAILED),
                false,
                None,
                None,
                false,
                false,
            ),
            Ok(TrashOutcomeQuery::OutcomeUnknown) | Err(_) => (
                ChangeItemState::Failed,
                Some(CHANGE_ITEM_OUTCOME_UNKNOWN),
                true,
                None,
                None,
                false,
                false,
            ),
        },
    }
}

fn same_object(left: &ChangeIdentity, right: &ChangeIdentity) -> bool {
    left.volume_id == right.volume_id && left.file_id == right.file_id
}

fn move_error(
    error: PortError,
) -> (
    ChangeItemState,
    Option<&'static str>,
    bool,
    Option<ChangeIdentity>,
    Option<TrashLocationRef>,
    bool,
    bool,
) {
    match error.kind {
        PortErrorKind::PermissionDenied => (
            ChangeItemState::Skipped,
            Some(CHANGE_ITEM_PERMISSION_DENIED),
            false,
            None,
            None,
            false,
            false,
        ),
        PortErrorKind::InUse => (
            ChangeItemState::Skipped,
            Some(CHANGE_ITEM_FILE_IN_USE),
            false,
            None,
            None,
            false,
            false,
        ),
        PortErrorKind::IdentityChanged => (
            ChangeItemState::Skipped,
            Some(CHANGE_ITEM_IDENTITY_CHANGED),
            false,
            None,
            None,
            false,
            false,
        ),
        _ if error.reason_key == "scan.disconnected" => (
            ChangeItemState::Failed,
            Some(CHANGE_ITEM_OS_FAILED),
            false,
            None,
            None,
            false,
            true,
        ),
        _ => (
            ChangeItemState::Failed,
            Some(CHANGE_ITEM_OS_FAILED),
            false,
            None,
            None,
            false,
            false,
        ),
    }
}
