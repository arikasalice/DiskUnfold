//! FR8.HD1.app-host.DD1 / FR8.HD1.app-db.DD1: upgrades from every earlier schema version
//! through the GUI startup entry, and FR3.HD1.app-db.DD1 recovery at that entry.
//!
//! Each older database is built with `AppDb::create_gui` (version 1) and the product's own
//! migration steps (`migration::apply_next`), so no binary fixture is committed.
use super::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The object each definition creates; index is the step number minus two. A table with the
/// same name in an older database makes exactly that step fail.
const BLOCKERS: [&str; 4] = [
    "app_db_purge_gate",
    "job_runtime_job_link",
    "user_env_recent_scope",
    "change_planner_ruleset",
];
const SETTING: &str = "200000";
const SCOPE: &str = "metadata: 1 root";

/// What a test compares before and after an upgrade or a restore.
#[derive(Debug, PartialEq)]
struct Content {
    version: u32,
    schema: Vec<(String, Option<String>)>,
    setting: Option<String>,
    records: Option<i64>,
    links: Option<i64>,
    scopes: Option<i64>,
}

fn read_only(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn count(db: &Connection, table: &str) -> rusqlite::Result<Option<i64>> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get(0).map(Some)
    })
}

fn content(path: &Path) -> Result<Content, Box<dyn std::error::Error>> {
    let db = read_only(path)?;
    let mut statement = db.prepare(
        "SELECT name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let schema = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    Ok(Content {
        version: db.pragma_query_value(None, "user_version", |row| row.get(0))?,
        schema,
        setting: db
            .query_row("SELECT value_json FROM user_env_setting", [], |row| {
                row.get(0)
            })
            .optional()?,
        records: count(&db, "job_runtime_job_record")?,
        links: count(&db, "job_runtime_job_link")?,
        scopes: count(&db, "user_env_recent_scope")?,
    })
}

/// Adds the rows that exist in the schema generation just installed.
fn seed(path: &Path, generation: u32) -> rusqlite::Result<()> {
    let db = Connection::open(path)?;
    match generation {
        1 => db.execute_batch(&format!(
            "INSERT INTO user_env_setting VALUES
                 ('space.shared_table_max_entries', '{SETTING}', 'screen', 1, 2, 1);
             UPDATE user_env_setting_version SET settings_version = 7;"
        )),
        2 => db.execute_batch(&format!(
            "INSERT INTO job_runtime_job_record VALUES
                 (x'00112233445566778899aabbccddeeff', 'scan.metadata', 1, '{SCOPE}', 'done',
                  1, 0, 0, 0, 0, NULL, NULL, NULL, 100, 200,
                  x'ffeeddccbbaa99887766554433221100', 200);"
        )),
        4 => db.execute_batch(
            "INSERT INTO user_env_recent_scope VALUES
                 (x'0102030405060708090a0b0c0d0e0f10', zeroblob(32), 1, '{\"include\":[]}',
                  1, 100, 100, 100, 1);",
        ),
        _ => Ok(()),
    }
}

/// Builds a database at `version` (1 to `HISTORY_SCHEMA_VERSION`) through the real steps.
fn old_database(dir: &Path, version: u32) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = dir.join(DATABASE_FILE);
    drop(AppDb::create_gui(&path).map_err(open_error)?);
    seed(&path, 1)?;
    let ordinary = tempfile::tempdir()?;
    for step in 2..=version {
        let backup = ordinary.path().join(format!("step-{step}.sqlite"));
        let applied = du_app_db::migration::apply_next(&path, &backup, &HISTORY_DEFINITIONS)
            .map_err(migration_error)?;
        assert_eq!(applied, step);
        seed(&path, step)?;
    }
    assert_eq!(content(&path)?.version, version);
    Ok(path)
}

fn names(dir: &Path) -> Result<Vec<String>, io::Error> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

#[test]
fn every_older_version_upgrades_with_a_verified_backup_and_keeps_its_data() -> TestResult {
    for version in 1..HISTORY_SCHEMA_VERSION {
        let dir = tempfile::tempdir()?;
        let path = old_database(dir.path(), version)?;
        let before = content(&path)?;
        assert_eq!(before.setting.as_deref(), Some(SETTING));

        let opened = open_with_status(dir.path())?;
        assert!(!opened.first_run, "version {version}");
        drop(opened.database);

        let after = content(&path)?;
        assert_eq!(after.version, HISTORY_SCHEMA_VERSION, "version {version}");
        assert_eq!(after.setting.as_deref(), Some(SETTING), "version {version}");
        // The record exists from schema 2; schema 3 mirrors it into the link table.
        let with_record = i64::from(version >= 2);
        assert_eq!(after.records, Some(with_record), "version {version}");
        assert_eq!(after.links, Some(with_record), "version {version}");
        assert_eq!(
            after.scopes,
            Some(i64::from(version >= 4)),
            "version {version}"
        );
        assert_eq!(
            count(&read_only(&path)?, "change_planner_ruleset")?,
            Some(0)
        );

        // One backup per definition; the first holds the database exactly as it was.
        let backups = dir.path().join("db-backups");
        let backup_dirs: Vec<_> = names(&backups)?
            .into_iter()
            .filter(|name| name.starts_with("backup-"))
            .collect();
        assert_eq!(
            backup_dirs.len(),
            (HISTORY_SCHEMA_VERSION - version) as usize,
            "version {version}"
        );
        let first = backups
            .join("backup-00000000000000000001")
            .join("database.sqlite");
        assert_eq!(content(&first)?, before, "version {version}");
        let check: String =
            read_only(&first)?.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        assert_eq!(check, "ok");
        assert!(!backups.join("migration-journal.json").exists());

        // A second start finds the current schema and takes no further backup.
        let reopened = open_with_status(dir.path())?;
        assert!(!reopened.first_run);
        drop(reopened.database);
        assert_eq!(names(&backups)?.len(), backup_dirs.len());
    }
    Ok(())
}

#[test]
fn the_current_version_starts_without_a_backup_and_without_changes() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = old_database(dir.path(), HISTORY_SCHEMA_VERSION)?;
    let before = content(&path)?;
    drop(open(dir.path())?);
    assert_eq!(content(&path)?, before);
    assert!(!dir.path().join("db-backups").exists());
    Ok(())
}

#[test]
fn a_failed_step_restores_the_whole_database_from_every_start_and_blocking_step() -> TestResult {
    for version in 2..HISTORY_SCHEMA_VERSION {
        for blocked_step in (version + 1)..=HISTORY_SCHEMA_VERSION {
            let label = format!("version {version}, blocked step {blocked_step}");
            let dir = tempfile::tempdir()?;
            let path = old_database(dir.path(), version)?;
            let blocker = BLOCKERS[(blocked_step - 2) as usize];
            Connection::open(&path)?.execute_batch(&format!(
                "CREATE TABLE {blocker} (note TEXT NOT NULL); INSERT INTO {blocker} VALUES ('keep');"
            ))?;
            let before = content(&path)?;

            // Steps before the blocked one commit, then the run is undone as a whole.
            for attempt in 0..2 {
                let error = open(dir.path())
                    .err()
                    .ok_or_else(|| format!("{label}: failing migration was accepted"))?;
                assert_eq!(
                    error.to_string(),
                    "startup.migration_failed: restored",
                    "{label}"
                );
                assert_eq!(content(&path)?, before, "{label}, attempt {attempt}");
            }
            let backups = dir.path().join("db-backups");
            assert!(
                backups.join("backup-00000000000000000001").is_dir(),
                "{label}"
            );
            assert!(
                names(&backups)?
                    .iter()
                    .any(|name| name.starts_with("restored-")),
                "{label}"
            );
            assert!(!backups.join("migration-journal.json").exists(), "{label}");

            // Removing the cause lets the next start upgrade the restored database.
            Connection::open(&path)?.execute_batch(&format!("DROP TABLE {blocker};"))?;
            drop(open(dir.path())?);
            let after = content(&path)?;
            assert_eq!(after.version, HISTORY_SCHEMA_VERSION, "{label}");
            assert_eq!(after.setting.as_deref(), Some(SETTING), "{label}");
        }
    }
    Ok(())
}

#[test]
fn a_version_1_database_with_unknown_objects_is_refused_before_any_backup() -> TestResult {
    // Startup checks the version 1 baseline first, so a failing step cannot be injected there.
    let dir = tempfile::tempdir()?;
    let path = old_database(dir.path(), 1)?;
    Connection::open(&path)?.execute_batch("CREATE TABLE app_db_purge_gate (note TEXT);")?;
    let before = content(&path)?;
    let error = open(dir.path())
        .err()
        .ok_or("version 1 database with unknown objects was accepted")?;
    assert!(error.to_string().contains("db.schema_mismatch"));
    assert_eq!(content(&path)?, before);
    // The empty backup folder may exist; no backup or journal may.
    let backups = dir.path().join("db-backups");
    assert!(!backups.exists() || names(&backups)?.is_empty());
    Ok(())
}

#[test]
fn a_newer_format_is_refused_and_never_modified() -> TestResult {
    for newer in [HISTORY_SCHEMA_VERSION + 1, HISTORY_SCHEMA_VERSION + 2, 999] {
        let dir = tempfile::tempdir()?;
        let path = old_database(dir.path(), HISTORY_SCHEMA_VERSION)?;
        Connection::open(&path)?.pragma_update(None, "user_version", newer)?;
        let before = fs::read(&path)?;

        let error = open(dir.path())
            .err()
            .ok_or_else(|| format!("version {newer} was accepted"))?;
        assert!(error.to_string().contains("db.schema_mismatch"), "{newer}");
        assert_eq!(fs::read(&path)?, before, "{newer}");
        assert!(!dir.path().join("db-backups").exists(), "{newer}");
        let leftovers: Vec<_> = names(dir.path())?
            .into_iter()
            .filter(|name| name.ends_with("-wal") || name.ends_with("-journal"))
            .collect();
        assert!(leftovers.is_empty(), "{newer}: {leftovers:?}");
    }
    Ok(())
}

#[test]
fn read_only_and_writer_opens_never_migrate_an_older_database() -> TestResult {
    for version in 1..HISTORY_SCHEMA_VERSION {
        let dir = tempfile::tempdir()?;
        let path = old_database(dir.path(), version)?;
        let bytes = fs::read(&path)?;
        let before = names(dir.path())?;

        assert_eq!(
            AppDb::probe_schema_version_read_only(&path).map_err(open_error)?,
            version
        );
        drop(AppDb::open_read_only(&path));
        // The read-only entries the CLI uses leave not even a side file behind.
        assert_eq!(fs::read(&path)?, bytes, "version {version}");
        assert_eq!(names(dir.path())?, before, "version {version}");
        // The GUI's live reader may create side files; no entry changes the database file.
        drop(AppDb::open_read_only_live(&path));
        // Writers that are not the startup migration refuse an older history schema.
        assert!(
            AppDb::open_gui_with_history(&path).is_err(),
            "version {version}"
        );
        assert!(
            AppDb::open_gui_with_history_after_version_probe(&path).is_err(),
            "version {version}"
        );

        assert_eq!(fs::read(&path)?, bytes, "version {version}");
        assert_eq!(content(&path)?.version, version);
    }
    Ok(())
}

fn complete_database_bytes(dir: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let path = dir.join("template.db");
    drop(AppDb::create_gui_with_history(&path).map_err(open_error)?);
    let bytes = fs::read(&path)?;
    fs::remove_file(&path)?;
    Ok(bytes)
}

#[test]
fn startup_recreates_a_database_whose_marker_route_creation_was_interrupted() -> TestResult {
    let template = tempfile::tempdir()?;
    let complete = complete_database_bytes(template.path())?;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(DATABASE_FILE);
    fs::write(&path, &complete[..complete.len() / 2])?;
    fs::write(dir.path().join(format!("{DATABASE_FILE}.creating")), b"")?;

    let opened = open_with_status(dir.path())?;
    assert!(opened.first_run);
    drop(opened.database);
    assert_eq!(content(&path)?.version, HISTORY_SCHEMA_VERSION);
    assert!(
        !dir.path()
            .join(format!("{DATABASE_FILE}.creating"))
            .exists()
    );
    Ok(())
}

#[test]
fn startup_keeps_a_database_whose_marker_was_written_before_the_interruption() -> TestResult {
    let template = tempfile::tempdir()?;
    let complete = complete_database_bytes(template.path())?;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(DATABASE_FILE);
    fs::write(&path, &complete)?;
    fs::write(
        dir.path().join(format!("{DATABASE_FILE}.created")),
        format!("{}\n", complete.len()),
    )?;
    fs::write(dir.path().join(format!("{DATABASE_FILE}.creating")), b"")?;

    let opened = open_with_status(dir.path())?;
    assert!(!opened.first_run);
    drop(opened.database);
    assert!(
        !dir.path()
            .join(format!("{DATABASE_FILE}.creating"))
            .exists()
    );
    Ok(())
}
