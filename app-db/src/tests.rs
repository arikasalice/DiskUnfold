use super::*;

#[test]
fn isolated_file_database_reopens_with_the_required_connection_settings()
-> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let path = sandbox.path().join("diskunfold.db");
    let first = AppDb::open(&path).map_err(|error| format!("{error:?}"))?;
    let second = AppDb::open(&path).map_err(|error| format!("{error:?}"))?;

    for db in [first, second] {
        let settings: (String, u32, u32, u32, u32) = db.connection.query_row(
            "SELECT (SELECT journal_mode FROM pragma_journal_mode),
                    (SELECT foreign_keys FROM pragma_foreign_keys),
                    (SELECT timeout FROM pragma_busy_timeout),
                    (SELECT synchronous FROM pragma_synchronous),
                    (SELECT application_id FROM pragma_application_id)",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        assert_eq!(settings, ("wal".into(), 1, 5_000, 1, APPLICATION_ID));
        assert_eq!(db.schema_version()?, SCHEMA_VERSION);
    }
    Ok(())
}

#[test]
fn rejects_foreign_or_newer_databases_without_rebranding_them()
-> Result<(), Box<dyn std::error::Error>> {
    let sandbox = tempfile::tempdir()?;
    let foreign_path = sandbox.path().join("other.db");
    let foreign = Connection::open(&foreign_path)?;
    foreign.execute("CREATE TABLE other (id INTEGER PRIMARY KEY)", [])?;
    assert!(matches!(
        AppDb::open(&foreign_path),
        Err(OpenError::ForeignDatabase)
    ));
    let id: u32 = foreign.pragma_query_value(None, "application_id", |row| row.get(0))?;
    assert_eq!(id, 0);

    let newer_path = sandbox.path().join("newer.db");
    let newer = Connection::open(&newer_path)?;
    newer.pragma_update(None, "application_id", APPLICATION_ID)?;
    newer.pragma_update(None, "user_version", 1)?;
    assert!(matches!(
        AppDb::open(&newer_path),
        Err(OpenError::UnsupportedSchema(1))
    ));
    Ok(())
}
