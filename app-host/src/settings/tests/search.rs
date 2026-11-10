//! Verify bilingual search against the SQLite snapshot restored at desktop startup.
use super::*;
use du_user_env::settings::search::Field;

#[test]
fn bilingual_search_reads_the_desktop_snapshot_without_writing()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("search.db");
    let mut database = AppDb::open_or_create_gui(&path).map_err(|error| format!("{error:?}"))?;
    let revision = database
        .commit(
            0,
            &SettingsChanges::from([(scope::REMEMBER_RECENT.into(), Some(json!(false)))]),
            context(),
        )
        .map_err(|error| format!("{error:?}"))?;
    drop(database);

    let resources = crate::locale::register()?;
    let languages = resources.configured_languages().collect::<Vec<_>>();
    let database = test_database(&path)?;
    let startup = StartupSettings::open(&database, 100, &languages)?;
    let service = startup.service.lock().map_err(|_| "settings unavailable")?;
    for (query, field) in [
        ("REMEMBER RECENT SCOPES", Field::Name("en")),
        ("최근 분석 범위 기억", Field::Name("ko")),
    ] {
        let result = service
            .search(&resources, query)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(result.revision, revision);
        assert!(!result.korean_integrity_degraded);
        assert_eq!(result.hits.len(), 1);
        let hit = &result.hits[0];
        assert_eq!(hit.entry.descriptor.key, scope::REMEMBER_RECENT);
        assert_eq!(hit.entry.value, &json!(false));
        assert!(!hit.entry.uses_default);
        assert!(hit.matches.iter().any(|found| found.field == field));
    }
    assert_eq!(service.catalog().0, revision);
    drop(service);
    drop(startup);
    drop(database);

    let reopened = open_settings(&path, 200)?;
    assert_eq!(reopened.snapshot().revision, revision);
    assert!(!scope::remember_recent(reopened.snapshot()));
    Ok(())
}
