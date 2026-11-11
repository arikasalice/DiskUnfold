//! Raw F29 ruleset persistence. Domain code validates format and hash.
use crate::AppDb;
use rusqlite::{OptionalExtension, TransactionBehavior, params};

/// An unverified persisted record. It must not be used as a protection decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesetRow {
    pub version: i64,
    pub ruleset_hash: Vec<u8>,
    pub hash_format: String,
    pub class_table_version: String,
    pub known_location_table_version: String,
    pub rule_format_version: i64,
    pub origin: String,
    pub change_summary: String,
    pub body: String,
    pub created_at_ms: i64,
}

/// Failure to read or append a ruleset row.
#[derive(Debug)]
pub enum RulesetError {
    /// The expected current version no longer matches.
    Stale,
    /// Stored fields are structurally invalid.
    Corrupt,
    /// SQLite rejected or could not perform the operation.
    Sqlite(rusqlite::Error),
}
impl From<rusqlite::Error> for RulesetError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}
const FIELDS: &str = "version, ruleset_hash, hash_format, class_table_version, known_location_table_version, rule_format_version, origin, change_summary, body, created_at_ms";
fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RulesetRow> {
    Ok(RulesetRow {
        version: row.get(0)?,
        ruleset_hash: row.get(1)?,
        hash_format: row.get(2)?,
        class_table_version: row.get(3)?,
        known_location_table_version: row.get(4)?,
        rule_format_version: row.get(5)?,
        origin: row.get(6)?,
        change_summary: row.get(7)?,
        body: row.get(8)?,
        created_at_ms: row.get(9)?,
    })
}
fn valid(row: &RulesetRow) -> bool {
    row.version > 0
        && row.ruleset_hash.len() == 32
        && !row.hash_format.is_empty()
        && !row.class_table_version.is_empty()
        && !row.known_location_table_version.is_empty()
        && row.rule_format_version > 0
        && matches!(row.origin.as_str(), "gui" | "import" | "product_update")
        && serde_json::from_str::<serde_json::Value>(&row.change_summary).is_ok()
        && serde_json::from_str::<serde_json::Value>(&row.body).is_ok()
}
impl AppDb {
    /// Reads the latest raw row. `None` means that no initial version exists.
    pub fn latest_ruleset(&self) -> Result<Option<RulesetRow>, RulesetError> {
        let sql =
            format!("SELECT {FIELDS} FROM change_planner_ruleset ORDER BY version DESC LIMIT 1");
        let row = self.connection.query_row(&sql, [], from_row).optional()?;
        if row.as_ref().is_some_and(|value| !valid(value)) {
            return Err(RulesetError::Corrupt);
        }
        Ok(row)
    }
    /// Reads a specific raw version without replacing it with a newer version.
    pub fn ruleset_version(&self, version: i64) -> Result<Option<RulesetRow>, RulesetError> {
        let sql = format!("SELECT {FIELDS} FROM change_planner_ruleset WHERE version = ?1");
        let row = self
            .connection
            .query_row(&sql, [version], from_row)
            .optional()?;
        if row.as_ref().is_some_and(|value| !valid(value)) {
            return Err(RulesetError::Corrupt);
        }
        Ok(row)
    }
    /// Appends one row under an immediate transaction if its expected base is current.
    ///
    /// `expected` is zero only when creating version one. The supplied row is
    /// raw: the domain caller must validate and compute its canonical hash.
    pub fn append_ruleset(&mut self, expected: i64, row: &RulesetRow) -> Result<(), RulesetError> {
        if !valid(row) {
            return Err(RulesetError::Corrupt);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: i64 = tx.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM change_planner_ruleset",
            [],
            |r| r.get(0),
        )?;
        if expected != current || current.checked_add(1) != Some(row.version) {
            return Err(RulesetError::Stale);
        }
        tx.execute("INSERT INTO change_planner_ruleset (version,ruleset_hash,hash_format,class_table_version,known_location_table_version,rule_format_version,origin,change_summary,body,created_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![row.version,row.ruleset_hash,row.hash_format,row.class_table_version,row.known_location_table_version,row.rule_format_version,row.origin,row.change_summary,row.body,row.created_at_ms])?;
        tx.commit()?;
        Ok(())
    }
}
#[cfg(test)]
mod tests;
