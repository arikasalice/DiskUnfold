//! Upgrading a representative version-six database adds the table and changes nothing else.

use super::super::SCOPE_OBSERVATION_SCHEMA_VERSION;
use super::*;
use crate::{HISTORY_DEFINITIONS, HISTORY_SCHEMA_VERSION, migration};
use rusqlite::{Connection, types::Value};

type Dump = Vec<(String, Vec<Vec<Value>>)>;

/// Every row of every table except the migration ledger, which gains one row by design.
fn dump(connection: &Connection) -> rusqlite::Result<Dump> {
    let mut tables = connection.prepare(
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
           AND name <> 'app_db_schema_meta' ORDER BY name",
    )?;
    let names = tables
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut dump = Vec::new();
    for name in names {
        let mut select = connection.prepare(&format!("SELECT * FROM {name}"))?;
        let columns = select.column_count();
        let mut rows = select
            .query_map([], |row| {
                (0..columns)
                    .map(|index| row.get::<_, Value>(index))
                    .collect()
            })?
            .collect::<rusqlite::Result<Vec<Vec<Value>>>>()?;
        rows.sort_by_key(|row| format!("{row:?}"));
        dump.push((name, rows));
    }
    Ok(dump)
}

fn seed_representative_rows(path: &std::path::Path) -> rusqlite::Result<()> {
    Connection::open(path)?.execute_batch(
        "INSERT INTO user_env_setting VALUES ('locale.display_language', '\"ko\"', 'screen', 1, 2, 1);
         UPDATE user_env_setting_version SET settings_version = 7;
         INSERT INTO job_runtime_job_record
            (job_id, kind_id, kind_version, scope_summary, final_state, completed_count,
             overwritten_count, failed_count, skipped_count, unprocessed_count,
             finished_at_ms, correlation_id, created_at_ms)
          VALUES (X'01010101010171018101010101010101', 'scan.metadata', 1, 'metadata: 1 root',
                  'done', 0, 0, 0, 0, 0, 900, X'01010101010171018101010101010101', 901);
         INSERT INTO job_runtime_job_link (job_id, record_origin, reckon_at_ms, record_seq, created_at_ms)
          VALUES (X'01010101010171018101010101010101', 'this_app', 900, 1, 901);
         UPDATE job_runtime_history_state SET last_record_seq = 1;
         INSERT INTO user_env_recent_scope
            (recent_scope_id, scope_fingerprint, scope_body, include_count,
             last_used_at_ms, created_at_ms, updated_at_ms)
          VALUES (X'02020202020270028202020202020202', zeroblob(32), '{}', 1, 5, 5, 5);
         INSERT INTO change_planner_ruleset VALUES
            (1, zeroblob(32), 'du.protect.ruleset.v1', '1', '1', 1, 'gui', '{}', '{}', 1);
         INSERT INTO analysis_index_group_ruleset
            (ruleset_id, format_version, version_no, revision, fingerprint, created_at_ms, updated_at_ms)
          VALUES (X'03030303030370038303030303030303', 1, 1, 1, zeroblob(32), 1, 1);",
    )
}

#[test]
fn version_six_data_is_unchanged_and_the_new_table_starts_empty() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = database_at(dir.path(), 6)?;
    seed_representative_rows(&path)?;
    let before = dump(&Connection::open(&path)?)?;
    assert!(
        before
            .iter()
            .any(|(name, rows)| name == "job_runtime_job_record" && rows.len() == 1)
    );
    let backup = dir.path().join("before-observation.db");
    assert_eq!(
        migration::apply_next(&path, &backup, &HISTORY_DEFINITIONS)
            .map_err(|e| format!("{e:?}"))?,
        SCOPE_OBSERVATION_SCHEMA_VERSION
    );
    assert_eq!(
        migration::apply_next(
            &path,
            &dir.path().join("before-scope-roots.db"),
            &HISTORY_DEFINITIONS
        )
        .map_err(|e| format!("{e:?}"))?,
        HISTORY_SCHEMA_VERSION
    );
    let upgraded = Connection::open(&path)?;
    let mut after = dump(&upgraded)?;
    let (table, rows) = after
        .iter()
        .find(|(name, _)| name == "analysis_index_scope_observation")
        .ok_or("the observation table was not created")?;
    assert_eq!(
        (table.as_str(), rows.len()),
        ("analysis_index_scope_observation", 0)
    );
    assert!(
        after
            .iter()
            .any(|(name, rows)| { name == "job_runtime_job_scope_root" && rows.is_empty() })
    );
    after.retain(|(name, _)| {
        name != "analysis_index_scope_observation" && name != "job_runtime_job_scope_root"
    });
    assert_eq!(after, before);
    let version: u32 = upgraded.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, HISTORY_SCHEMA_VERSION);
    let ledger: i64 = upgraded.query_row(
        "SELECT max(migration_no) FROM app_db_schema_meta",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(ledger as u32, HISTORY_SCHEMA_VERSION);
    // The pre-upgrade backup is a complete version-six copy without the table.
    let old = Connection::open(&backup)?;
    let old_tables: i64 = old.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name = 'analysis_index_scope_observation'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!((old_tables, dump(&old)?), (0, before));
    // The upgraded database opens for writing and accepts its first observation.
    drop(upgraded);
    let mut db = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        db.append_scope_observation(&observation(1, 1, 1_000)),
        Ok(ScopeObservationAppend::Added)
    );
    Ok(())
}
