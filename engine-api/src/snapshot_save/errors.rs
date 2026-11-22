use du_analysis_index::{CompletedScanError, QueryError};
use du_core_contracts::{
    snapshot_file::{SnapshotFileError, SnapshotPublishError},
    snapshot_index::SnapshotIndexError,
};
use du_job_runtime::RuntimeError;

use crate::error::EngineError;

use super::SaveFailure;

pub(super) fn result_error(error: QueryError) -> EngineError {
    match error {
        QueryError::NotFound | QueryError::StaleVersion => {
            EngineError::reason("result.version_not_found")
        }
        _ => EngineError::reason("support.internal_error"),
    }
}

pub(super) fn completed_error(error: CompletedScanError) -> EngineError {
    match error {
        CompletedScanError::Unconfirmed | CompletedScanError::Lookup(QueryError::NotFound) => {
            EngineError::reason("snapshot.result_not_complete")
        }
        CompletedScanError::Lookup(error) => result_error(error),
    }
}

pub(super) fn index_error(error: SnapshotIndexError) -> EngineError {
    match error {
        SnapshotIndexError::InvalidInput | SnapshotIndexError::Corrupt => {
            EngineError::reason("support.internal_error")
        }
        SnapshotIndexError::UnsupportedSchema => EngineError::reason("db.schema_mismatch"),
        SnapshotIndexError::Busy => EngineError::reason("db.busy"),
        SnapshotIndexError::Storage | SnapshotIndexError::StaleCursor => {
            EngineError::reason("snapshot.store_unavailable")
        }
    }
}

pub(super) fn runtime_error(error: RuntimeError) -> EngineError {
    match error {
        RuntimeError::QueueFull | RuntimeError::DeliveryBusy => {
            EngineError::reason("job.queue_full")
        }
        RuntimeError::ShuttingDown => EngineError::reason("job.shutting_down"),
        _ => EngineError::reason("support.internal_error"),
    }
}

pub(super) fn file_error(error: SnapshotFileError) -> EngineError {
    match error {
        SnapshotFileError::StoreUnavailable | SnapshotFileError::Unsupported => {
            EngineError::reason("snapshot.store_unavailable")
        }
        SnapshotFileError::NotFound => EngineError::reason("snapshot.not_found"),
        SnapshotFileError::ReadFailed(_) => EngineError::reason("snapshot.store_unavailable"),
        SnapshotFileError::InsufficientSpace => EngineError::reason_with_internal(
            "snapshot.store_unavailable",
            "snapshot.insufficient_space",
        ),
        SnapshotFileError::WriteFailed(_) => EngineError::reason("snapshot.write_failed"),
    }
}

pub(super) fn save_file_error(error: SnapshotFileError) -> SaveFailure {
    SaveFailure::Failed(match error {
        SnapshotFileError::StoreUnavailable | SnapshotFileError::Unsupported => {
            "snapshot.store_unavailable"
        }
        SnapshotFileError::NotFound => "snapshot.not_found",
        SnapshotFileError::ReadFailed(_) => "snapshot.store_unavailable",
        SnapshotFileError::InsufficientSpace => "snapshot.insufficient_space",
        SnapshotFileError::WriteFailed(_) => "snapshot.write_failed",
    })
}

pub(super) fn publish_error(error: SnapshotPublishError) -> SaveFailure {
    match error {
        SnapshotPublishError::WriteFailed(_) | SnapshotPublishError::RegisteredNotDurable(_) => {
            SaveFailure::Failed("snapshot.write_failed")
        }
    }
}

pub(super) fn serializer_error(error: du_snapshot_store::serializer::Error) -> SaveFailure {
    match error {
        du_snapshot_store::serializer::Error::Cancelled => SaveFailure::Cancelled,
        _ => SaveFailure::Failed("snapshot.write_failed"),
    }
}
