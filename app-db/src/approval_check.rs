use crate::AppDb;
use du_core_contracts::{
    Id,
    approval_check::{
        ApprovalCheckFacts, ApprovalCheckFinishRecord, ApprovalCheckRecord,
        ApprovalCheckRecordKind, ApprovalCheckRecorder, ApprovalCheckStartRecord,
        ApprovalOperationKind, ApprovalRequestKind,
    },
    ports::{PortError, PortErrorKind},
};
use rusqlite::{Row, TransactionBehavior, params, types::Type};
use std::io::{Error as IoError, ErrorKind};

const COLUMNS: &str = "check_id, job_id, request_version_id, request_kind, version_hash,
 hash_format, operation_kind, destination_summary, keep_summary, record_kind,
 check_result, mismatch_fields, end_state, approved_at_ms, created_at_ms, correlation_id";

impl ApprovalCheckRecorder for AppDb {
    fn append_start(&mut self, record: &ApprovalCheckStartRecord) -> Result<(), PortError> {
        validate(&record.facts)?;
        self.append_record(&record.facts, "start", None)
    }

    fn append_finish(&mut self, record: &ApprovalCheckFinishRecord) -> Result<(), PortError> {
        validate(&record.facts)?;
        if record.end_state.is_empty() {
            return Err(port_error(PortErrorKind::Corrupt, "db.corrupt"));
        }
        self.append_record(&record.facts, "finish", Some(&record.end_state))
    }

    fn read_for_request(
        &self,
        request_version_id: &Id,
    ) -> Result<Vec<ApprovalCheckRecord>, PortError> {
        let id = id_blob(request_version_id)?;
        self.read_records(
            &format!(
                "SELECT {COLUMNS} FROM job_runtime_approval_check
                 WHERE request_version_id = ?1 ORDER BY created_at_ms"
            ),
            id,
        )
    }

    fn read_for_job(&self, job_id: &Id) -> Result<Vec<ApprovalCheckRecord>, PortError> {
        let id = id_blob(job_id)?;
        self.read_records(
            &format!(
                "SELECT {COLUMNS} FROM job_runtime_approval_check
                 WHERE job_id = ?1 ORDER BY created_at_ms"
            ),
            id,
        )
    }
}

impl AppDb {
    fn append_record(
        &mut self,
        facts: &ApprovalCheckFacts,
        record_kind: &str,
        end_state: Option<&str>,
    ) -> Result<(), PortError> {
        let check_id = id_blob(&facts.check_id)?;
        let job_id = facts.job_id.as_ref().map(id_blob).transpose()?;
        let request_version_id = id_blob(&facts.request_version_id)?;
        let correlation_id = id_blob(&facts.correlation_id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        tx.execute(
            "INSERT INTO job_runtime_approval_check
             (check_id, job_id, request_version_id, request_kind, version_hash, hash_format,
              operation_kind, destination_summary, keep_summary, record_kind, check_result,
              mismatch_fields, end_state, approved_at_ms, created_at_ms, correlation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                check_id,
                job_id,
                request_version_id,
                request_kind_name(facts.request_kind),
                facts.version_hash.as_slice(),
                facts.hash_format,
                operation_kind_name(facts.operation_kind),
                facts.destination_summary,
                facts.keep_summary,
                record_kind,
                facts.check_result,
                facts.mismatch_fields,
                end_state,
                facts.approved_at_ms,
                facts.created_at_ms,
                correlation_id,
            ],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)
    }

    fn read_records(&self, sql: &str, key: Vec<u8>) -> Result<Vec<ApprovalCheckRecord>, PortError> {
        let mut statement = self.connection.prepare(sql).map_err(db_error)?;
        let rows = statement.query_map([key], from_row).map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }
}

fn validate(facts: &ApprovalCheckFacts) -> Result<(), PortError> {
    if facts.hash_format.is_empty()
        || facts.check_result.is_empty()
        || !is_json(&facts.mismatch_fields, true)
        || facts
            .destination_summary
            .as_deref()
            .is_some_and(|value| !is_json(value, false))
        || facts
            .keep_summary
            .as_deref()
            .is_some_and(|value| !is_json(value, false))
    {
        return Err(port_error(PortErrorKind::Corrupt, "db.corrupt"));
    }
    Ok(())
}

fn is_json(value: &str, array_only: bool) -> bool {
    serde_json::from_str::<serde_json::Value>(value)
        .is_ok_and(|json| !array_only || matches!(json, serde_json::Value::Array(_)))
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<ApprovalCheckRecord> {
    let check_id = id_from_blob(row.get(0)?, 0)?;
    let job_id = row
        .get::<_, Option<Vec<u8>>>(1)?
        .map(|value| id_from_blob(value, 1))
        .transpose()?;
    let request_version_id = id_from_blob(row.get(2)?, 2)?;
    let request_kind = request_kind_from_name(&row.get::<_, String>(3)?, 3)?;
    let hash: Vec<u8> = row.get(4)?;
    let version_hash = hash
        .try_into()
        .map_err(|_| blob_error(4, "invalid hash length"))?;
    let hash_format: String = row.get(5)?;
    let operation_kind = operation_kind_from_name(&row.get::<_, String>(6)?, 6)?;
    let destination_summary: Option<String> = row.get(7)?;
    let keep_summary: Option<String> = row.get(8)?;
    let record_kind = record_kind_from_name(&row.get::<_, String>(9)?, 9)?;
    let check_result: String = row.get(10)?;
    let mismatch_fields: String = row.get(11)?;
    let end_state: Option<String> = row.get(12)?;
    let approved_at_ms = row.get(13)?;
    let created_at_ms = row.get(14)?;
    let correlation_id = id_from_blob(row.get(15)?, 15)?;
    let facts = ApprovalCheckFacts {
        check_id,
        job_id,
        request_version_id,
        request_kind,
        version_hash,
        hash_format,
        operation_kind,
        destination_summary,
        keep_summary,
        check_result,
        mismatch_fields,
        approved_at_ms,
        created_at_ms,
        correlation_id,
    };
    if validate(&facts).is_err()
        || match record_kind {
            ApprovalCheckRecordKind::Start => end_state.is_some(),
            ApprovalCheckRecordKind::Finish => end_state.as_deref().is_none_or(str::is_empty),
        }
    {
        return Err(text_error(10, "invalid approval check row"));
    }
    Ok(ApprovalCheckRecord {
        facts,
        record_kind,
        end_state,
    })
}

fn request_kind_name(value: ApprovalRequestKind) -> &'static str {
    match value {
        ApprovalRequestKind::Plan => "plan",
        ApprovalRequestKind::Restore => "restore",
        ApprovalRequestKind::MoveUndo => "move_undo",
    }
}

fn request_kind_from_name(value: &str, index: usize) -> rusqlite::Result<ApprovalRequestKind> {
    match value {
        "plan" => Ok(ApprovalRequestKind::Plan),
        "restore" => Ok(ApprovalRequestKind::Restore),
        "move_undo" => Ok(ApprovalRequestKind::MoveUndo),
        _ => Err(text_error(index, "invalid request kind")),
    }
}

fn operation_kind_name(value: ApprovalOperationKind) -> &'static str {
    match value {
        ApprovalOperationKind::Trash => "trash",
        ApprovalOperationKind::Move => "move",
        ApprovalOperationKind::Delete => "delete",
        ApprovalOperationKind::Restore => "restore",
        ApprovalOperationKind::MoveUndo => "move_undo",
    }
}

fn operation_kind_from_name(value: &str, index: usize) -> rusqlite::Result<ApprovalOperationKind> {
    match value {
        "trash" => Ok(ApprovalOperationKind::Trash),
        "move" => Ok(ApprovalOperationKind::Move),
        "delete" => Ok(ApprovalOperationKind::Delete),
        "restore" => Ok(ApprovalOperationKind::Restore),
        "move_undo" => Ok(ApprovalOperationKind::MoveUndo),
        _ => Err(text_error(index, "invalid operation kind")),
    }
}

fn record_kind_from_name(value: &str, index: usize) -> rusqlite::Result<ApprovalCheckRecordKind> {
    match value {
        "start" => Ok(ApprovalCheckRecordKind::Start),
        "finish" => Ok(ApprovalCheckRecordKind::Finish),
        _ => Err(text_error(index, "invalid record kind")),
    }
}

fn id_blob(id: &Id) -> Result<Vec<u8>, PortError> {
    id.v7_bytes()
        .map(|value| value.to_vec())
        .ok_or_else(|| port_error(PortErrorKind::Corrupt, "db.corrupt"))
}

fn id_from_blob(bytes: Vec<u8>, index: usize) -> rusqlite::Result<Id> {
    let bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| blob_error(index, "invalid UUID length"))?;
    let value = uuid::Uuid::from_bytes(bytes).to_string();
    Id::parse(&value).map_err(|_| blob_error(index, "invalid UUID version"))
}

fn blob_error(index: usize, message: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Blob,
        Box::new(IoError::new(ErrorKind::InvalidData, message.to_owned())),
    )
}

fn text_error(index: usize, message: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Text,
        Box::new(IoError::new(ErrorKind::InvalidData, message.to_owned())),
    )
}

fn db_error(error: rusqlite::Error) -> PortError {
    let (kind, reason) = match &error {
        rusqlite::Error::SqliteFailure(failure, _) => match failure.code {
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => {
                (PortErrorKind::Busy, "db.busy")
            }
            rusqlite::ErrorCode::ConstraintViolation
            | rusqlite::ErrorCode::DatabaseCorrupt
            | rusqlite::ErrorCode::NotADatabase => (PortErrorKind::Corrupt, "db.corrupt"),
            _ => (PortErrorKind::Io, "db.storage_unavailable"),
        },
        rusqlite::Error::InvalidColumnType(..)
        | rusqlite::Error::IntegralValueOutOfRange(..)
        | rusqlite::Error::FromSqlConversionFailure(..) => (PortErrorKind::Corrupt, "db.corrupt"),
        _ => (PortErrorKind::Io, "db.storage_unavailable"),
    };
    port_error(kind, reason)
}

fn port_error(kind: PortErrorKind, reason_key: &str) -> PortError {
    PortError {
        kind,
        reason_key: reason_key.to_owned(),
        os_code: None,
    }
}

#[cfg(test)]
mod tests;
