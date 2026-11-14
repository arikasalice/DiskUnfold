//! F36 virtual-group rule storage (FR1.HD2.app-db.DD1, IF D-01).
//!
//! The adapter persists the single group ruleset as one header row and its rule
//! rows. It checks the optimistic write revision and the structure of the stored
//! values; version numbers, fingerprints and rule meaning belong to the caller.

use du_core_contracts::{
    Id,
    group_rules::{
        GroupRulesReadError, GroupRulesSaveOutcome, StoredGroupRule, StoredGroupRuleset,
    },
};
use rusqlite::{ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::AppDb;

/// A header row with its integer columns still in SQLite's 64-bit form.
struct RawHead {
    ruleset_id: Vec<u8>,
    format_version: i64,
    version_no: i64,
    revision: i64,
    fingerprint: Vec<u8>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

/// A rule row with its integer columns still in SQLite's 64-bit form.
struct RawRule {
    group_id: Vec<u8>,
    display_name: String,
    display_order: i64,
    condition_json: String,
    condition_model_version: i64,
    predicates_json: String,
}

impl AppDb {
    /// Reads the single stored group ruleset from one deferred transaction.
    ///
    /// Returns `None` when no header row exists. Rules are ordered by `display_order`.
    /// Structurally invalid stored values (lengths, negative numbers, malformed JSON,
    /// identifiers that are not UUID v7) yield `Corrupt`; lock contention yields `Busy`
    /// and any other SQLite failure yields `Unavailable`.
    pub fn read_group_ruleset(&self) -> Result<Option<StoredGroupRuleset>, GroupRulesReadError> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(read_error)?;
        let head = tx
            .query_row(
                "SELECT ruleset_id, format_version, version_no, revision, fingerprint,
                        created_at_ms, updated_at_ms
                 FROM analysis_index_group_ruleset LIMIT 1",
                [],
                |row| {
                    Ok(RawHead {
                        ruleset_id: row.get(0)?,
                        format_version: row.get(1)?,
                        version_no: row.get(2)?,
                        revision: row.get(3)?,
                        fingerprint: row.get(4)?,
                        created_at_ms: row.get(5)?,
                        updated_at_ms: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(read_error)?;
        let Some(head) = head else {
            return Ok(None);
        };
        let raw_id: [u8; 16] = head
            .ruleset_id
            .as_slice()
            .try_into()
            .map_err(|_| GroupRulesReadError::Corrupt)?;
        let rules = read_rules(&tx, &raw_id)?;
        Ok(Some(StoredGroupRuleset {
            ruleset_id: stored_id(&head.ruleset_id)?,
            format_version: positive_u32(head.format_version)?,
            version_no: positive_u32(head.version_no)?,
            revision: positive_u32(head.revision)?,
            fingerprint: head
                .fingerprint
                .try_into()
                .map_err(|_| GroupRulesReadError::Corrupt)?,
            created_at_ms: head.created_at_ms,
            updated_at_ms: head.updated_at_ms,
            rules,
        }))
    }

    /// Replaces the whole stored ruleset if the stored revision equals `expected_revision`.
    ///
    /// Zero means that no ruleset exists yet. The header update, the removal of the old
    /// rule rows and the insertion of `next.rules` commit in one immediate transaction.
    /// A revision mismatch reports `Conflict` and writes nothing; lock contention reports
    /// `Busy`; any other failure, including input that is structurally invalid or a ruleset
    /// identity that differs from the stored one, rolls back and reports `Failed`.
    /// The call is never retried. The caller computes `version_no`, `revision` and the
    /// fingerprint.
    pub fn replace_group_ruleset(
        &mut self,
        expected_revision: u32,
        next: &StoredGroupRuleset,
    ) -> GroupRulesSaveOutcome {
        let Some(encoded) = Encoded::new(next) else {
            return GroupRulesSaveOutcome::Failed;
        };
        let tx = match self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
        {
            Ok(tx) => tx,
            Err(error) => return save_failure(&error),
        };
        match write_ruleset(&tx, expected_revision, &encoded) {
            Ok(Replaced::Written) => match tx.commit() {
                Ok(()) => GroupRulesSaveOutcome::Saved,
                Err(error) => save_failure(&error),
            },
            Ok(Replaced::Conflict(current_revision)) => {
                // Nothing was written; a failed rollback only leaves the drop to finish it.
                let _ = tx.rollback();
                GroupRulesSaveOutcome::Conflict { current_revision }
            }
            Ok(Replaced::Rejected) => GroupRulesSaveOutcome::Failed,
            Err(error) => save_failure(&error),
        }
    }
}

fn read_rules(
    tx: &Transaction<'_>,
    ruleset_id: &[u8; 16],
) -> Result<Vec<StoredGroupRule>, GroupRulesReadError> {
    let mut statement = tx
        .prepare(
            "SELECT group_id, display_name, display_order, condition_json,
                    condition_model_version, predicates_json
             FROM analysis_index_group_rule WHERE ruleset_id = ?1 ORDER BY display_order",
        )
        .map_err(read_error)?;
    let rows = statement
        .query_map([ruleset_id.as_slice()], |row| {
            Ok(RawRule {
                group_id: row.get(0)?,
                display_name: row.get(1)?,
                display_order: row.get(2)?,
                condition_json: row.get(3)?,
                condition_model_version: row.get(4)?,
                predicates_json: row.get(5)?,
            })
        })
        .map_err(read_error)?;
    let mut rules = Vec::new();
    for row in rows {
        rules.push(rule_from_raw(row.map_err(read_error)?)?);
    }
    Ok(rules)
}

fn rule_from_raw(raw: RawRule) -> Result<StoredGroupRule, GroupRulesReadError> {
    if !is_json(&raw.condition_json) || !is_json(&raw.predicates_json) {
        return Err(GroupRulesReadError::Corrupt);
    }
    Ok(StoredGroupRule {
        group_id: stored_id(&raw.group_id)?,
        display_name: raw.display_name,
        display_order: u32::try_from(raw.display_order)
            .map_err(|_| GroupRulesReadError::Corrupt)?,
        condition_json: raw.condition_json,
        condition_model_version: positive_u32(raw.condition_model_version)?,
        predicates_json: raw.predicates_json,
    })
}

fn stored_id(bytes: &[u8]) -> Result<Id, GroupRulesReadError> {
    let bytes: [u8; 16] = bytes.try_into().map_err(|_| GroupRulesReadError::Corrupt)?;
    let id = uuid::Uuid::from_bytes(bytes);
    if id.get_version_num() != 7 || id.get_variant() != uuid::Variant::RFC4122 {
        return Err(GroupRulesReadError::Corrupt);
    }
    Ok(Id::from_v7_bytes(bytes))
}

fn positive_u32(value: i64) -> Result<u32, GroupRulesReadError> {
    u32::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(GroupRulesReadError::Corrupt)
}

fn is_json(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text).is_ok()
}

fn is_lock_failure(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

fn read_error(error: rusqlite::Error) -> GroupRulesReadError {
    match error {
        _ if is_lock_failure(&error) => GroupRulesReadError::Busy,
        rusqlite::Error::InvalidColumnType(..)
        | rusqlite::Error::IntegralValueOutOfRange(..)
        | rusqlite::Error::Utf8Error(..)
        | rusqlite::Error::FromSqlConversionFailure(..) => GroupRulesReadError::Corrupt,
        _ => GroupRulesReadError::Unavailable,
    }
}

fn save_failure(error: &rusqlite::Error) -> GroupRulesSaveOutcome {
    if is_lock_failure(error) {
        GroupRulesSaveOutcome::Busy
    } else {
        GroupRulesSaveOutcome::Failed
    }
}

/// The input ruleset with its identifiers converted to stored bytes.
struct Encoded<'a> {
    ruleset_id: [u8; 16],
    group_ids: Vec<[u8; 16]>,
    next: &'a StoredGroupRuleset,
}

impl<'a> Encoded<'a> {
    /// Returns `None` when the input would not satisfy the schema's structural rules.
    fn new(next: &'a StoredGroupRuleset) -> Option<Self> {
        if next.format_version == 0 || next.version_no == 0 || next.revision == 0 {
            return None;
        }
        let mut group_ids = Vec::with_capacity(next.rules.len());
        for rule in &next.rules {
            if rule.condition_model_version == 0
                || !is_json(&rule.condition_json)
                || !is_json(&rule.predicates_json)
            {
                return None;
            }
            group_ids.push(rule.group_id.v7_bytes()?);
        }
        Some(Self {
            ruleset_id: next.ruleset_id.v7_bytes()?,
            group_ids,
            next,
        })
    }
}

/// How one replacement attempt ended before the commit.
enum Replaced {
    Written,
    Conflict(u32),
    Rejected,
}

fn write_ruleset(
    tx: &Transaction<'_>,
    expected_revision: u32,
    encoded: &Encoded<'_>,
) -> rusqlite::Result<Replaced> {
    let next = encoded.next;
    let updated = tx.execute(
        "UPDATE analysis_index_group_ruleset
         SET format_version = ?1, version_no = ?2, revision = ?3, fingerprint = ?4,
             updated_at_ms = ?5
         WHERE ruleset_id = ?6 AND revision = ?7",
        params![
            next.format_version,
            next.version_no,
            next.revision,
            next.fingerprint.as_slice(),
            next.updated_at_ms,
            encoded.ruleset_id.as_slice(),
            expected_revision,
        ],
    )?;
    if updated == 0 {
        let stored: Option<(Vec<u8>, i64)> = tx
            .query_row(
                "SELECT ruleset_id, revision FROM analysis_index_group_ruleset LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match stored {
            None if expected_revision == 0 => insert_head(tx, encoded)?,
            None => return Ok(Replaced::Conflict(0)),
            Some((id, revision)) => {
                let Ok(revision) = u32::try_from(revision) else {
                    return Ok(Replaced::Rejected);
                };
                if revision != expected_revision {
                    return Ok(Replaced::Conflict(revision));
                }
                if id != encoded.ruleset_id {
                    return Ok(Replaced::Rejected);
                }
            }
        }
    }
    tx.execute(
        "DELETE FROM analysis_index_group_rule WHERE ruleset_id = ?1",
        [encoded.ruleset_id.as_slice()],
    )?;
    insert_rules(tx, encoded)?;
    Ok(Replaced::Written)
}

fn insert_head(tx: &Transaction<'_>, encoded: &Encoded<'_>) -> rusqlite::Result<()> {
    let next = encoded.next;
    tx.execute(
        "INSERT INTO analysis_index_group_ruleset
            (ruleset_id, format_version, version_no, revision, fingerprint,
             created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            encoded.ruleset_id.as_slice(),
            next.format_version,
            next.version_no,
            next.revision,
            next.fingerprint.as_slice(),
            next.created_at_ms,
            next.updated_at_ms,
        ],
    )?;
    Ok(())
}

fn insert_rules(tx: &Transaction<'_>, encoded: &Encoded<'_>) -> rusqlite::Result<()> {
    let mut statement = tx.prepare(
        "INSERT INTO analysis_index_group_rule
            (rule_row_id, ruleset_id, group_id, display_name, display_order, condition_json,
             condition_model_version, predicates_json, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for (rule, group_id) in encoded.next.rules.iter().zip(&encoded.group_ids) {
        statement.execute(params![
            uuid::Uuid::now_v7().as_bytes().as_slice(),
            encoded.ruleset_id.as_slice(),
            group_id.as_slice(),
            rule.display_name,
            rule.display_order,
            rule.condition_json,
            rule.condition_model_version,
            rule.predicates_json,
            encoded.next.updated_at_ms,
        ])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
