use du_core_contracts::{
    Id,
    group_rules::{
        GroupRulesReadError, GroupRulesSaveOutcome, StoredGroupRule, StoredGroupRuleset,
    },
};
use rusqlite::Connection;
use std::{path::PathBuf, time::Duration};

use crate::{AppDb, HISTORY_DEFINITIONS, HISTORY_SCHEMA_VERSION, migration};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn rule(seed: u8, order: u32) -> StoredGroupRule {
    StoredGroupRule {
        group_id: Id::from_v7_bytes([seed; 16]),
        display_name: format!("group {seed}"),
        display_order: order,
        condition_json: format!("{{\"kind\":\"test\",\"seed\":{seed}}}"),
        condition_model_version: 1,
        predicates_json: "[{\"id\":\"size.over\",\"revision\":1}]".into(),
    }
}

fn ruleset(revision: u32, rules: Vec<StoredGroupRule>) -> StoredGroupRuleset {
    StoredGroupRuleset {
        ruleset_id: Id::from_v7_bytes([9; 16]),
        format_version: 1,
        version_no: revision,
        revision,
        fingerprint: [revision as u8; 32],
        created_at_ms: 100,
        updated_at_ms: 100 + i64::from(revision),
        rules,
    }
}

fn new_db() -> Result<(tempfile::TempDir, PathBuf, AppDb), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("rules.db");
    let db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    Ok((dir, path, db))
}

fn rule_rows(db: &AppDb) -> rusqlite::Result<i64> {
    db.connection
        .query_row("SELECT count(*) FROM analysis_index_group_rule", [], |r| {
            r.get(0)
        })
}

#[test]
fn empty_database_reads_none_and_first_save_round_trips_in_order() -> TestResult {
    let (_dir, _path, mut db) = new_db()?;
    assert_eq!(db.read_group_ruleset(), Ok(None));
    let first = ruleset(1, vec![rule(3, 2), rule(1, 0), rule(2, 1)]);
    assert_eq!(
        db.replace_group_ruleset(0, &first),
        GroupRulesSaveOutcome::Saved
    );
    let stored = db
        .read_group_ruleset()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing")?;
    let orders: Vec<_> = stored.rules.iter().map(|r| r.display_order).collect();
    assert_eq!(orders, [0, 1, 2]);
    let mut expected = first;
    expected.rules.sort_by_key(|r| r.display_order);
    assert_eq!(stored, expected);
    Ok(())
}

#[test]
fn empty_rule_list_is_stored_and_read_back() -> TestResult {
    let (_dir, _path, mut db) = new_db()?;
    let empty = ruleset(1, Vec::new());
    assert_eq!(
        db.replace_group_ruleset(0, &empty),
        GroupRulesSaveOutcome::Saved
    );
    assert_eq!(db.read_group_ruleset(), Ok(Some(empty)));
    Ok(())
}

#[test]
fn mismatched_expected_revision_conflicts_and_leaves_content_unchanged() -> TestResult {
    let (_dir, _path, mut db) = new_db()?;
    let other = ruleset(6, vec![rule(1, 0)]);
    assert_eq!(
        db.replace_group_ruleset(5, &other),
        GroupRulesSaveOutcome::Conflict {
            current_revision: 0
        }
    );
    assert_eq!(db.read_group_ruleset(), Ok(None));
    let first = ruleset(1, vec![rule(1, 0)]);
    assert_eq!(
        db.replace_group_ruleset(0, &first),
        GroupRulesSaveOutcome::Saved
    );
    for expected in [0, 2, 7] {
        let second = ruleset(expected + 1, vec![rule(2, 0)]);
        assert_eq!(
            db.replace_group_ruleset(expected, &second),
            GroupRulesSaveOutcome::Conflict {
                current_revision: 1
            }
        );
    }
    assert_eq!(db.read_group_ruleset(), Ok(Some(first)));
    Ok(())
}

#[test]
fn a_revision_that_does_not_advance_by_one_writes_nothing() -> TestResult {
    let (_dir, _path, mut db) = new_db()?;
    let first = ruleset(1, vec![rule(1, 0)]);
    assert_eq!(
        db.replace_group_ruleset(0, &first),
        GroupRulesSaveOutcome::Saved
    );
    for next in [ruleset(1, vec![rule(2, 0)]), ruleset(3, vec![rule(2, 0)])] {
        assert_eq!(
            db.replace_group_ruleset(1, &next),
            GroupRulesSaveOutcome::Failed
        );
    }
    assert_eq!(
        db.replace_group_ruleset(u32::MAX, &first),
        GroupRulesSaveOutcome::Failed
    );
    assert_eq!(db.read_group_ruleset(), Ok(Some(first)));
    Ok(())
}

#[test]
fn the_schema_holds_at_most_one_header_row() -> TestResult {
    let (_dir, _path, mut db) = new_db()?;
    let first = ruleset(1, vec![rule(1, 0)]);
    assert_eq!(
        db.replace_group_ruleset(0, &first),
        GroupRulesSaveOutcome::Saved
    );
    let second = db.connection.execute(
        "INSERT INTO analysis_index_group_ruleset
            (ruleset_id, format_version, version_no, revision, fingerprint,
             created_at_ms, updated_at_ms)
         VALUES (?1, 1, 1, 1, zeroblob(32), 1, 1)",
        [vec![7u8; 16]],
    );
    assert!(second.is_err(), "a second header row was accepted");
    assert_eq!(db.read_group_ruleset(), Ok(Some(first)));
    Ok(())
}

#[test]
fn an_unsupported_format_version_is_returned_as_stored() -> TestResult {
    let (_dir, _path, mut db) = new_db()?;
    let mut future = ruleset(1, vec![rule(1, 0)]);
    future.format_version = 2;
    assert_eq!(
        db.replace_group_ruleset(0, &future),
        GroupRulesSaveOutcome::Saved
    );
    // The adapter does not interpret formats; the engine refuses formats it does not define.
    assert_eq!(db.read_group_ruleset(), Ok(Some(future)));
    Ok(())
}

#[test]
fn replacement_removes_the_previous_rule_rows() -> TestResult {
    let (_dir, _path, mut db) = new_db()?;
    let first = ruleset(1, vec![rule(1, 0), rule(2, 1), rule(3, 2)]);
    assert_eq!(
        db.replace_group_ruleset(0, &first),
        GroupRulesSaveOutcome::Saved
    );
    assert_eq!(rule_rows(&db)?, 3);
    let second = ruleset(2, vec![rule(4, 0)]);
    assert_eq!(
        db.replace_group_ruleset(1, &second),
        GroupRulesSaveOutcome::Saved
    );
    assert_eq!(rule_rows(&db)?, 1);
    assert_eq!(db.read_group_ruleset(), Ok(Some(second)));
    let third = ruleset(3, Vec::new());
    assert_eq!(
        db.replace_group_ruleset(2, &third),
        GroupRulesSaveOutcome::Saved
    );
    assert_eq!(rule_rows(&db)?, 0);
    assert_eq!(db.read_group_ruleset(), Ok(Some(third)));
    Ok(())
}

#[test]
fn two_connections_saving_with_the_same_expected_revision_save_once() -> TestResult {
    let (_dir, path, mut first_db) = new_db()?;
    let mut second_db = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let base = ruleset(1, vec![rule(1, 0)]);
    assert_eq!(
        first_db.replace_group_ruleset(0, &base),
        GroupRulesSaveOutcome::Saved
    );
    let from_first = ruleset(2, vec![rule(2, 0)]);
    let from_second = ruleset(2, vec![rule(3, 0)]);
    assert_eq!(
        first_db.replace_group_ruleset(1, &from_first),
        GroupRulesSaveOutcome::Saved
    );
    assert_eq!(
        second_db.replace_group_ruleset(1, &from_second),
        GroupRulesSaveOutcome::Conflict {
            current_revision: 2
        }
    );
    assert_eq!(second_db.read_group_ruleset(), Ok(Some(from_first)));
    Ok(())
}

#[test]
fn failure_in_the_middle_rolls_back_to_the_previous_version() -> TestResult {
    let (_dir, _path, mut db) = new_db()?;
    let first = ruleset(1, vec![rule(1, 0), rule(2, 1)]);
    assert_eq!(
        db.replace_group_ruleset(0, &first),
        GroupRulesSaveOutcome::Saved
    );
    let duplicate_group = ruleset(2, vec![rule(5, 0), rule(5, 1)]);
    assert_eq!(
        db.replace_group_ruleset(1, &duplicate_group),
        GroupRulesSaveOutcome::Failed
    );
    let duplicate_order = ruleset(2, vec![rule(5, 0), rule(6, 0)]);
    assert_eq!(
        db.replace_group_ruleset(1, &duplicate_order),
        GroupRulesSaveOutcome::Failed
    );
    assert_eq!(db.read_group_ruleset(), Ok(Some(first.clone())));
    let mut other_identity = ruleset(2, vec![rule(7, 0)]);
    other_identity.ruleset_id = Id::from_v7_bytes([8; 16]);
    assert_eq!(
        db.replace_group_ruleset(1, &other_identity),
        GroupRulesSaveOutcome::Failed
    );
    let mut zero_revision = ruleset(2, Vec::new());
    zero_revision.revision = 0;
    assert_eq!(
        db.replace_group_ruleset(1, &zero_revision),
        GroupRulesSaveOutcome::Failed
    );
    let mut bad_json = ruleset(2, vec![rule(7, 0)]);
    bad_json.rules[0].condition_json = "{".into();
    assert_eq!(
        db.replace_group_ruleset(1, &bad_json),
        GroupRulesSaveOutcome::Failed
    );
    assert_eq!(db.read_group_ruleset(), Ok(Some(first)));
    Ok(())
}

#[test]
fn locked_database_reports_busy_without_writing() -> TestResult {
    let (_dir, path, mut db) = new_db()?;
    db.connection.busy_timeout(Duration::ZERO)?;
    let holder = Connection::open(&path)?;
    holder.execute_batch("BEGIN IMMEDIATE")?;
    let next = ruleset(1, vec![rule(1, 0)]);
    assert_eq!(
        db.replace_group_ruleset(0, &next),
        GroupRulesSaveOutcome::Busy
    );
    holder.execute_batch("ROLLBACK")?;
    assert_eq!(db.read_group_ruleset(), Ok(None));
    Ok(())
}

fn corrupt_with(db: &AppDb, sql: &str) -> rusqlite::Result<()> {
    db.connection.execute_batch(&format!(
        "PRAGMA foreign_keys=OFF; PRAGMA ignore_check_constraints=ON; {sql};
         PRAGMA ignore_check_constraints=OFF; PRAGMA foreign_keys=ON;"
    ))
}

#[test]
fn structurally_invalid_rows_read_as_corrupt() -> TestResult {
    let damage = [
        "UPDATE analysis_index_group_ruleset SET fingerprint = zeroblob(31)",
        "UPDATE analysis_index_group_ruleset SET revision = 0",
        "UPDATE analysis_index_group_ruleset SET version_no = -1",
        "UPDATE analysis_index_group_ruleset SET format_version = 4294967296",
        "UPDATE analysis_index_group_ruleset SET ruleset_id = zeroblob(15)",
        "UPDATE analysis_index_group_rule SET group_id = zeroblob(16)",
        "UPDATE analysis_index_group_rule SET display_order = -1",
        "UPDATE analysis_index_group_rule SET condition_json = '{'",
        "UPDATE analysis_index_group_rule SET predicates_json = 'nope'",
        "UPDATE analysis_index_group_rule SET condition_model_version = 0",
    ];
    for sql in damage {
        let (_dir, _path, mut db) = new_db()?;
        let stored = ruleset(1, vec![rule(1, 0)]);
        assert_eq!(
            db.replace_group_ruleset(0, &stored),
            GroupRulesSaveOutcome::Saved
        );
        assert_eq!(db.read_group_ruleset(), Ok(Some(stored)), "{sql}");
        corrupt_with(&db, sql)?;
        assert_eq!(
            db.read_group_ruleset(),
            Err(GroupRulesReadError::Corrupt),
            "{sql}"
        );
    }
    Ok(())
}

#[test]
fn missing_tables_read_as_unavailable() -> TestResult {
    let (_dir, _path, db) = new_db()?;
    db.connection.execute_batch(
        "DROP TABLE analysis_index_group_rule; DROP TABLE analysis_index_group_ruleset;",
    )?;
    assert_eq!(
        db.read_group_ruleset(),
        Err(GroupRulesReadError::Unavailable)
    );
    Ok(())
}

#[test]
fn version_five_database_upgrades_to_six_and_keeps_existing_data() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.db");
    drop(AppDb::create_gui(&path).map_err(|e| format!("{e:?}"))?);
    let before_group_rules = HISTORY_SCHEMA_VERSION - 1;
    for step in HISTORY_DEFINITIONS
        .steps
        .iter()
        .filter(|step| step.number <= before_group_rules)
    {
        let backup = dir.path().join(format!("before-{}.db", step.number));
        assert_eq!(
            migration::apply_next(&path, &backup, &HISTORY_DEFINITIONS)
                .map_err(|e| format!("{e:?}"))?,
            step.number
        );
    }
    let old = Connection::open(&path)?;
    old.execute_batch(
        "INSERT INTO user_env_setting VALUES ('locale.display_language', '\"ko\"', 'screen', 1, 2, 1);
         UPDATE user_env_setting_version SET settings_version = 7;
         INSERT INTO change_planner_ruleset VALUES
            (1, zeroblob(32), 'du.protect.ruleset.v1', '1', '1', 1, 'gui', '{}', '{}', 1);",
    )?;
    let has_tables: i64 = old.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name LIKE 'analysis_index_group_%'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(has_tables, 0);
    drop(old);
    assert!(matches!(
        AppDb::open_gui_with_history(&path),
        Err(crate::OpenError::HistorySchema(_))
    ));
    let backup = dir.path().join("before-group-rules.db");
    assert_eq!(
        migration::apply_next(&path, &backup, &HISTORY_DEFINITIONS)
            .map_err(|e| format!("{e:?}"))?,
        6
    );
    let mut db = AppDb::open_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    assert_eq!(db.schema_version()?, 6);
    let kept: (String, i64, i64) = db.connection.query_row(
        "SELECT (SELECT value_json FROM user_env_setting WHERE setting_key = 'locale.display_language'),
                (SELECT settings_version FROM user_env_setting_version),
                (SELECT count(*) FROM change_planner_ruleset)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(kept, ("\"ko\"".into(), 7, 1));
    let tables: i64 = db.connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name IN
            ('analysis_index_group_ruleset', 'analysis_index_group_rule')",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(tables, 2);
    assert_eq!(db.read_group_ruleset(), Ok(None));
    let first = ruleset(1, vec![rule(1, 0)]);
    assert_eq!(
        db.replace_group_ruleset(0, &first),
        GroupRulesSaveOutcome::Saved
    );
    assert_eq!(db.read_group_ruleset(), Ok(Some(first)));
    let previous = AppDb::open_read_only(&backup).map_err(|e| format!("{e:?}"))?;
    assert_eq!(previous.schema_version()?, 5);
    Ok(())
}
