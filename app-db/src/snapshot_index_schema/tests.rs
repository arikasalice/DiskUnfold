use crate::{AppDb, HISTORY_DEFINITIONS, HISTORY_SCHEMA_VERSION, migration};
use rusqlite::Connection;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn list_state(connection: &Connection) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut statement =
        connection.prepare("SELECT name, list_seq FROM snapshot_store_list_state ORDER BY name")?;
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

#[test]
fn fresh_and_latest_older_migrations_create_one_snapshot_list_state_row() -> TestResult {
    let dir = tempfile::tempdir()?;
    let fresh_path = dir.path().join("fresh.db");
    let fresh =
        AppDb::create_gui_with_history(&fresh_path).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        list_state(&fresh.connection)?,
        vec![("snapshot_list".into(), 0)]
    );

    let empty_path = dir.path().join("empty-upgrade.db");
    drop(AppDb::create_gui(&empty_path).map_err(|error| format!("{error:?}"))?);
    for definition in HISTORY_DEFINITIONS.steps {
        migration::apply_next(
            &empty_path,
            &dir.path()
                .join(format!("empty-before-{}.db", definition.number)),
            &HISTORY_DEFINITIONS,
        )
        .map_err(|error| format!("{error:?}"))?;
    }
    let empty_upgrade = Connection::open(&empty_path)?;
    assert_eq!(
        list_state(&empty_upgrade)?,
        vec![("snapshot_list".into(), 0)]
    );

    let older_path = dir.path().join("older.db");
    drop(AppDb::create_gui(&older_path).map_err(|error| format!("{error:?}"))?);
    for definition in HISTORY_DEFINITIONS
        .steps
        .iter()
        .filter(|definition| definition.number < HISTORY_SCHEMA_VERSION)
    {
        migration::apply_next(
            &older_path,
            &dir.path().join(format!("before-{}.db", definition.number)),
            &HISTORY_DEFINITIONS,
        )
        .map_err(|error| format!("{error:?}"))?;
    }
    let before = Connection::open(&older_path)?;
    let version: u32 = before.pragma_query_value(None, "user_version", |row| row.get(0))?;
    assert_eq!(version, HISTORY_SCHEMA_VERSION - 1);
    drop(before);

    assert_eq!(
        migration::apply_next(
            &older_path,
            &dir.path().join("before-snapshot-index.db"),
            &HISTORY_DEFINITIONS,
        )
        .map_err(|error| format!("{error:?}"))?,
        HISTORY_SCHEMA_VERSION
    );
    let migrated = Connection::open(&older_path)?;
    assert_eq!(list_state(&migrated)?, vec![("snapshot_list".into(), 0)]);
    assert_eq!(
        migrated.query_row("SELECT count(*) FROM snapshot_store_snapshot", [], |row| {
            row.get::<_, i64>(0)
        },)?,
        0
    );
    Ok(())
}
