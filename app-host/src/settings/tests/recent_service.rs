//! Test-only composition of the F01 policy and the real SQLite adapter.
use super::*;
use du_app_db::recent_scope::{
    RecentError as DbError, RecentItem, RecentScopeStore, RecentSummary, RecentWrite,
};
use du_core_contracts::settings::{SETTINGS_SCHEMA_VERSION, SettingsSnapshot};
use du_user_env::scope;
use du_user_env::scope::recent::{
    RecentBody, RecentError, RecentRepository, RecentService, RepositoryError,
};
use std::num::NonZeroUsize;

struct Candidate(RecentWrite<'static>);
impl RecentBody for Candidate {
    fn body(&self) -> &str {
        self.0.body
    }
}
struct SqliteFixture {
    db: AppDb,
}
fn map(error: DbError) -> RepositoryError {
    match error {
        DbError::Busy => RepositoryError::Busy,
        DbError::DiskFull => RepositoryError::Full,
        DbError::Corrupt => RepositoryError::Corrupt,
        DbError::Invalid | DbError::Storage => RepositoryError::Storage,
    }
}
impl RecentRepository for SqliteFixture {
    type Summary = RecentSummary;
    type Item = RecentItem;
    type Write = Candidate;
    fn list(&mut self, cap: NonZeroUsize) -> Result<Vec<Self::Summary>, RepositoryError> {
        RecentScopeStore::new(&mut self.db, cap, &valid)
            .map_err(map)?
            .list()
            .map_err(map)
    }
    fn get(
        &mut self,
        id: &[u8; 16],
        cap: NonZeroUsize,
        validate: &dyn Fn(&str) -> bool,
    ) -> Result<Option<Self::Item>, RepositoryError> {
        RecentScopeStore::new(&mut self.db, cap, validate)
            .map_err(map)?
            .get(id)
            .map_err(map)
    }
    fn put(
        &mut self,
        item: &Candidate,
        cap: NonZeroUsize,
        validate: &dyn Fn(&str) -> bool,
    ) -> Result<(), RepositoryError> {
        RecentScopeStore::new(&mut self.db, cap, validate)
            .map_err(map)?
            .put(&item.0)
            .map_err(map)
    }
}
fn valid(body: &str) -> bool {
    body == r#"{"fixture":true}"#
}
fn id(number: u8) -> [u8; 16] {
    let mut result = [0; 16];
    result[6] = 0x70;
    result[8] = 0x80;
    result[15] = number;
    result
}
fn candidate(number: u8, body: &'static str) -> Candidate {
    Candidate(RecentWrite {
        id: id(number),
        fingerprint: [number; 32],
        body,
        include_count: 1,
        now_ms: i64::from(number),
    })
}

#[test]
fn policy_and_sqlite_preserve_existing_rows_and_distinguish_failures()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("fixture.db");
    let db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let cap = NonZeroUsize::new(1).ok_or("zero")?;
    let mut service = RecentService::new(SqliteFixture { db }, cap, valid);
    let mut snapshot = SettingsSnapshot {
        schema_version: SETTINGS_SCHEMA_VERSION,
        ..SettingsSnapshot::default()
    };
    service
        .put(Some(&snapshot), &candidate(1, r#"{"fixture":true}"#))
        .map_err(|e| format!("{e:?}"))?;
    snapshot
        .values
        .insert(scope::REMEMBER_RECENT.into(), json!(false));
    // Off skips even validation and leaves the already stored row readable.
    assert_eq!(
        service.put(Some(&snapshot), &candidate(2, "invalid")),
        Ok(())
    );
    let mut refresh = candidate(1, r#"{"fixture":true}"#);
    refresh.0.now_ms = 99;
    assert_eq!(service.put(Some(&snapshot), &refresh), Ok(()));
    assert_eq!(
        service.put(Some(&snapshot), &candidate(2, r#"{"fixture":true}"#)),
        Ok(())
    );
    assert_eq!(service.list().map_err(|e| format!("{e:?}"))?.len(), 1);
    let original = service
        .get(&id(1))
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing original")?;
    assert_eq!(original.summary.last_used_at_ms, 1);
    assert_eq!(service.get(&id(2)).map_err(|e| format!("{e:?}"))?, None);
    snapshot
        .values
        .insert(scope::REMEMBER_RECENT.into(), json!(true));
    assert_eq!(
        service.put(Some(&snapshot), &candidate(2, "invalid")),
        Err(RecentError::InvalidBody)
    );
    // The number 1 is a fixture limit, not a published product default.
    service
        .put(None, &candidate(2, r#"{"fixture":true}"#))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(service.list().map_err(|e| format!("{e:?}"))?.len(), 1);
    assert!(service.get(&id(1)).map_err(|e| format!("{e:?}"))?.is_none());
    let db = Connection::open(&path)?;
    db.execute("UPDATE user_env_recent_scope SET scope_body='{}'", [])?;
    assert_eq!(
        service.get(&id(2)),
        Err(RecentError::Repository(RepositoryError::Corrupt))
    );
    db.execute("DROP TABLE user_env_recent_scope", [])?;
    assert_eq!(
        service.list(),
        Err(RecentError::Repository(RepositoryError::Storage))
    );
    Ok(())
}
