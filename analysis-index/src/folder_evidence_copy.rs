//! Fallible exact event copying for immutable result-version evidence.
use crate::folder_evidence_api::{FolderAttemptFinal, FolderAttemptInput, FolderAttemptToken};
use du_core_contracts::{
    ports::PortError,
    scan_observation::{
        CorrelatedDirectoryEvent, MetadataObservation, ObservedEntry, ObservedValue, SpaceIdentity,
        TimedObservedEntry,
    },
};

pub(crate) fn try_clone_input(input: &FolderAttemptInput) -> Option<FolderAttemptInput> {
    Some(match input {
        FolderAttemptInput::Adapter(event) => {
            FolderAttemptInput::Adapter(try_clone_adapter_event(event)?)
        }
        FolderAttemptInput::Final { child_id, outcome } => FolderAttemptInput::Final {
            child_id: *child_id,
            outcome: try_clone_final(*outcome),
        },
        FolderAttemptInput::PendingCleared { child_id } => FolderAttemptInput::PendingCleared {
            child_id: *child_id,
        },
        FolderAttemptInput::TransportFailure => FolderAttemptInput::TransportFailure,
        FolderAttemptInput::Cancelled => FolderAttemptInput::Cancelled,
    })
}

fn try_clone_adapter_event(event: &CorrelatedDirectoryEvent) -> Option<CorrelatedDirectoryEvent> {
    Some(match event {
        CorrelatedDirectoryEvent::FilterCallback {
            child_id,
            raw_name,
            decision,
            phase,
        } => CorrelatedDirectoryEvent::FilterCallback {
            child_id: *child_id,
            raw_name: try_clone_bytes(raw_name)?,
            decision: *decision,
            phase: *phase,
        },
        CorrelatedDirectoryEvent::Observation { child_id, entry } => {
            CorrelatedDirectoryEvent::Observation {
                child_id: *child_id,
                entry: try_clone_timed_entry(entry)?,
            }
        }
        CorrelatedDirectoryEvent::Progress => CorrelatedDirectoryEvent::Progress,
        CorrelatedDirectoryEvent::End {
            child_count_exclusive,
        } => CorrelatedDirectoryEvent::End {
            child_count_exclusive: *child_count_exclusive,
        },
    })
}

fn try_clone_timed_entry(entry: &TimedObservedEntry) -> Option<TimedObservedEntry> {
    Some(TimedObservedEntry {
        entry: try_clone_observed_entry(&entry.entry)?,
        observed_at: entry.observed_at.clone(),
        placeholder_observation: entry.placeholder_observation,
    })
}

fn try_clone_observed_entry(entry: &ObservedEntry) -> Option<ObservedEntry> {
    Some(ObservedEntry {
        name: try_clone_bytes(&entry.name)?,
        enumerated_inode: entry.enumerated_inode,
        enumerated_kind: entry.enumerated_kind.clone(),
        metadata: match &entry.metadata {
            Ok(metadata) => Ok(try_clone_metadata(metadata)?),
            Err(error) => Err(try_clone_error(error)?),
        },
    })
}

fn try_clone_metadata(metadata: &MetadataObservation) -> Option<MetadataObservation> {
    Some(MetadataObservation {
        kind: metadata.kind.clone(),
        device: metadata.device,
        inode: metadata.inode,
        size: match &metadata.size {
            Ok(size) => Ok(*size),
            Err(error) => Err(try_clone_error(error)?),
        },
        modified_at: metadata.modified_at.clone(),
        native_modified_at: metadata.native_modified_at,
        allocated: metadata.allocated,
        link_count: metadata.link_count,
        space_identity: match &metadata.space_identity {
            ObservedValue::Known(identity) => ObservedValue::Known(try_clone_identity(identity)?),
            ObservedValue::Unknown(reason) => ObservedValue::Unknown(reason),
            ObservedValue::NotApplicable => ObservedValue::NotApplicable,
        },
    })
}

fn try_clone_identity(identity: &SpaceIdentity) -> Option<SpaceIdentity> {
    Some(SpaceIdentity {
        volume: try_clone_bytes(&identity.volume)?,
        file: try_clone_bytes(&identity.file)?,
    })
}

fn try_clone_error(error: &PortError) -> Option<PortError> {
    Some(PortError {
        kind: error.kind.clone(),
        reason_key: try_clone_string(&error.reason_key)?,
        os_code: error.os_code,
    })
}

fn try_clone_final(final_outcome: FolderAttemptFinal) -> FolderAttemptFinal {
    match final_outcome {
        FolderAttemptFinal::Recorded { child_attempt } => FolderAttemptFinal::Recorded {
            child_attempt: child_attempt.map(try_clone_token),
        },
        FolderAttemptFinal::Boundary => FolderAttemptFinal::Boundary,
        FolderAttemptFinal::Revisit => FolderAttemptFinal::Revisit,
        FolderAttemptFinal::Unresolved => FolderAttemptFinal::Unresolved,
    }
}

fn try_clone_token(token: FolderAttemptToken) -> FolderAttemptToken {
    token
}

fn try_clone_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len()).ok()?;
    copy.extend_from_slice(bytes);
    Some(copy)
}

fn try_clone_string(value: &str) -> Option<String> {
    let mut copy = String::new();
    copy.try_reserve_exact(value.len()).ok()?;
    copy.push_str(value);
    Some(copy)
}
