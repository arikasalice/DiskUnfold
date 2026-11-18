use super::super::{
    folder_attempt_contract::{
        CallbackPhase, CallbackSeen, ChildCorrelationCapability, DirectoryAttemptEvent,
        DirectoryAttemptKey, DirectoryChildFinal,
    },
    folder_attempt_history::HistoryBudget,
};
use super::*;
use crate::observation::StoredObservation;
use du_core_contracts::{result::ResultItemKind, scan_observation::ObservedKind};
use std::sync::Arc;

type AttemptTestResult = Result<(), String>;

fn store() -> Result<ObservationStore, String> {
    let mut store = ObservationStore::default();
    store
        .insert(
            0,
            &[],
            StoredObservation {
                size: None,
                kind: ResultItemKind::Directory,
                physical: None,
                modified_at: None,
                native_modified_at: None,
                enumerated_inode: None,
                placeholder_observation: None,
                observed_at: None,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    Ok(store)
}

fn key() -> DirectoryAttemptKey {
    DirectoryAttemptKey {
        build_id: 7,
        root_index: 0,
        raw_parent: vec![],
        attempt_id: 3,
    }
}

fn start(
    ledger: &mut AttemptLedger,
    store: &ObservationStore,
    correlation: Option<ChildCorrelationCapability>,
) {
    ledger.record(
        DirectoryAttemptEvent::Started {
            key: key(),
            event_seq: 0,
            correlation,
        },
        store,
    );
}

fn seen(
    event_seq: u64,
    callback_id: u64,
    child_id: Option<u64>,
    name: &[u8],
    decision: du_core_contracts::scan_observation::FilterDecision,
    phase: CallbackPhase,
) -> DirectoryAttemptEvent {
    DirectoryAttemptEvent::CallbackSeen(CallbackSeen {
        key: key(),
        event_seq,
        callback_id,
        child_id,
        raw_name: name.to_vec(),
        decision,
        phase,
    })
}

fn final_event(
    event_seq: u64,
    child_id: u64,
    classification: DirectoryChildFinal,
) -> DirectoryAttemptEvent {
    DirectoryAttemptEvent::ChildFinal {
        key: key(),
        event_seq,
        child_id,
        classification,
    }
}

fn end(
    event_watermark_exclusive: u64,
    callback_watermark_exclusive: u64,
    child_count_exclusive: u64,
) -> DirectoryAttemptEvent {
    DirectoryAttemptEvent::End {
        key: key(),
        event_watermark_exclusive,
        callback_watermark_exclusive,
        child_count_exclusive,
    }
}

#[path = "folder_attempt_correlation_tests.rs"]
mod correlation;
#[path = "folder_attempt_failure_tests.rs"]
mod failure;
#[path = "folder_attempt_history_tests.rs"]
mod history;
#[path = "folder_attempt_protocol_tests.rs"]
mod protocol;
#[path = "folder_attempt_unresolved_tests.rs"]
mod unresolved;
