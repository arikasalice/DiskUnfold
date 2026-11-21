use crate::AppDb;
use du_core_contracts::{
    Id,
    data_source::DataSource,
    plan_version_store::{PlanOriginChannel, PlanReadiness, PlanVersionRecord, PlanVersionStore},
    ports::{PortError, PortErrorKind},
};
use rusqlite::{OptionalExtension, Row, TransactionBehavior, params, types::Type};
use std::io::{Error as IoError, ErrorKind};

/// Maximum serialized plan body accepted by the storage adapter.
///
/// The plan builder allows at most 100,000 items. Budgeting 1.25 KiB per item,
/// including a path of up to about 1 KiB, gives about 122.1 MiB; this rounds up
/// to 128 MiB for the stored body.
const MAX_PLAN_BODY_BYTES: usize = 128 * 1024 * 1024;
const COLUMNS: &str = "plan_version_id, lineage_id, parent_plan_version_id,
 version_hash, hash_format, ruleset_version, readiness, origin_channel, body, created_at_ms";

impl PlanVersionStore for AppDb {
    fn append_fixed(&mut self, version: &PlanVersionRecord) -> Result<(), PortError> {
        validate(version)?;
        let plan_id = id_blob(&version.plan_version_id)?;
        let lineage_id = id_blob(&version.lineage_id)?;
        let parent_id = version
            .parent_plan_version_id
            .as_ref()
            .map(id_blob)
            .transpose()?;
        let ruleset_version = i64::try_from(version.ruleset_version).map_err(|_| input_error())?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let already_exists: bool = tx
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM change_planner_plan_version
                 WHERE plan_version_id = ?1)",
                [plan_id.as_slice()],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if already_exists {
            return Err(port_error(PortErrorKind::InUse, "db.duplicate"));
        }
        tx.execute(
            "INSERT INTO change_planner_plan_version
             (plan_version_id, lineage_id, parent_plan_version_id, version_hash,
              hash_format, ruleset_version, readiness, origin_channel, body, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                plan_id,
                lineage_id,
                parent_id,
                version.version_hash.as_slice(),
                version.hash_format,
                ruleset_version,
                readiness_name(version.readiness),
                channel_name(version.origin_channel),
                version.body,
                version.created_at_ms,
            ],
        )
        .map_err(db_write_error)?;
        tx.commit().map_err(db_error)
    }

    fn read(&self, version_id: &Id) -> Result<PlanVersionRecord, PortError> {
        let id = id_blob(version_id)?;
        self.connection
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM change_planner_plan_version WHERE plan_version_id = ?1"
                ),
                [id],
                from_row,
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| port_error(PortErrorKind::NotFound, "plan.version_not_found"))
    }

    fn read_lineage(&self, lineage_id: &Id) -> Result<Vec<PlanVersionRecord>, PortError> {
        let id = id_blob(lineage_id)?;
        let sql = format!(
            "SELECT {COLUMNS} FROM change_planner_plan_version
             WHERE lineage_id = ?1 ORDER BY created_at_ms, plan_version_id"
        );
        let mut statement = self.connection.prepare(&sql).map_err(db_error)?;
        let rows = statement.query_map([id], from_row).map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }
}

/// Validates a plan version at the storage boundary.
///
/// The plan builder must reject an oversized plan first with a `plan.*` reason.
/// Reaching this guard with an oversized plan is a caller bug: `db.invalid_input`
/// is internal-only (`INTERNAL` on the wire), and `db.duplicate` is not exposed
/// as a user error.
fn validate(version: &PlanVersionRecord) -> Result<(), PortError> {
    if version.data_source == DataSource::Sample {
        return Err(port_error(
            PortErrorKind::Unsupported,
            "plan.sample_not_stored",
        ));
    }
    if version.ruleset_version == 0
        || i64::try_from(version.ruleset_version).is_err()
        || version.hash_format.is_empty()
        || version.body.len() > MAX_PLAN_BODY_BYTES
        || serde_json::from_str::<serde_json::Value>(&version.body).is_err()
    {
        return Err(input_error());
    }
    Ok(())
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<PlanVersionRecord> {
    let plan_version_id = id_from_blob(row.get(0)?, 0)?;
    let lineage_id = id_from_blob(row.get(1)?, 1)?;
    let parent_plan_version_id = row
        .get::<_, Option<Vec<u8>>>(2)?
        .map(|value| id_from_blob(value, 2))
        .transpose()?;
    let hash: Vec<u8> = row.get(3)?;
    let version_hash = hash.try_into().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            Type::Blob,
            Box::new(IoError::new(ErrorKind::InvalidData, "invalid hash length")),
        )
    })?;
    let hash_format: String = row.get(4)?;
    let ruleset_version: i64 = row.get(5)?;
    let ruleset_version = u64::try_from(ruleset_version).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            Type::Integer,
            Box::new(IoError::new(
                ErrorKind::InvalidData,
                "invalid ruleset version",
            )),
        )
    })?;
    let readiness = readiness_from_name(&row.get::<_, String>(6)?, 6)?;
    let origin_channel = channel_from_name(&row.get::<_, String>(7)?, 7)?;
    let body: String = row.get(8)?;
    if hash_format.is_empty()
        || body.len() > MAX_PLAN_BODY_BYTES
        || serde_json::from_str::<serde_json::Value>(&body).is_err()
    {
        return Err(rusqlite::Error::FromSqlConversionFailure(
            8,
            Type::Text,
            Box::new(IoError::new(ErrorKind::InvalidData, "invalid plan body")),
        ));
    }
    Ok(PlanVersionRecord {
        plan_version_id,
        lineage_id,
        parent_plan_version_id,
        version_hash,
        hash_format,
        ruleset_version,
        readiness,
        origin_channel,
        body,
        created_at_ms: row.get(9)?,
        data_source: DataSource::Real,
    })
}

fn id_blob(id: &Id) -> Result<Vec<u8>, PortError> {
    id.v7_bytes()
        .map(|value| value.to_vec())
        .ok_or_else(input_error)
}

fn id_from_blob(bytes: Vec<u8>, index: usize) -> rusqlite::Result<Id> {
    let bytes: [u8; 16] = bytes.try_into().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Blob,
            Box::new(IoError::new(ErrorKind::InvalidData, "invalid UUID length")),
        )
    })?;
    let value = uuid::Uuid::from_bytes(bytes).to_string();
    Id::parse(&value).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Blob,
            Box::new(IoError::new(ErrorKind::InvalidData, "invalid UUID version")),
        )
    })
}

fn readiness_name(value: PlanReadiness) -> &'static str {
    match value {
        PlanReadiness::Ready => "ready",
        PlanReadiness::NeedsConfirmation => "needs_confirmation",
        PlanReadiness::NoTarget => "no_target",
    }
}

fn readiness_from_name(value: &str, index: usize) -> rusqlite::Result<PlanReadiness> {
    match value {
        "ready" => Ok(PlanReadiness::Ready),
        "needs_confirmation" => Ok(PlanReadiness::NeedsConfirmation),
        "no_target" => Ok(PlanReadiness::NoTarget),
        _ => Err(text_error(index, "invalid readiness")),
    }
}

fn channel_name(value: PlanOriginChannel) -> &'static str {
    match value {
        PlanOriginChannel::Gui => "gui",
        PlanOriginChannel::Cli => "cli",
        PlanOriginChannel::Mcp => "mcp",
    }
}

fn channel_from_name(value: &str, index: usize) -> rusqlite::Result<PlanOriginChannel> {
    match value {
        "gui" => Ok(PlanOriginChannel::Gui),
        "cli" => Ok(PlanOriginChannel::Cli),
        "mcp" => Ok(PlanOriginChannel::Mcp),
        _ => Err(text_error(index, "invalid origin channel")),
    }
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

fn db_write_error(error: rusqlite::Error) -> PortError {
    if matches!(
        &error,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::ConstraintViolation
    ) {
        input_error()
    } else {
        db_error(error)
    }
}

fn input_error() -> PortError {
    // PortErrorKind has no invalid-input variant; keep the distinction in the reason key.
    port_error(PortErrorKind::Unsupported, "db.invalid_input")
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
