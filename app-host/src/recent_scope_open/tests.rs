use super::*;
use du_app_db::AppDb;
use du_core_contracts::{
    ErrorCode, Id, OsPath, PathBytes, PathEncoding, ResultEnvelope,
    analysis_ipc::{ScopeBoundary, ScopeDraft, ScopeSource},
    ports::{PortError, PortErrorKind},
    scan_observation::OpenedScopeRoot,
    volume::{RecentVolumeEntry, RecentVolumeObservation},
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Default)]
struct Roots(AtomicUsize);

impl ScopeRootPort for Roots {
    fn open_root(&self, _: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(PortError {
            kind: PortErrorKind::PermissionDenied,
            reason_key: "os.query_failed".into(),
            os_code: None,
        })
    }
}

struct Volumes {
    evidence: Option<RecentVolumeObservation>,
    calls: AtomicUsize,
}

impl RecentVolumePort for Volumes {
    fn recent_volume(
        &self,
        _: &[u8],
        _: &OsPath,
        continue_work: &dyn Fn() -> bool,
    ) -> Option<RecentVolumeObservation> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if continue_work() {
            self.evidence.clone()
        } else {
            None
        }
    }
}

fn path(text: &str) -> OsPath {
    let raw: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    OsPath {
        encoding: PathEncoding::WindowsUtf16le,
        raw: PathBytes::from_bytes(&raw),
        display: text.into(),
        lossy: false,
    }
}

fn id(number: u8) -> Id {
    Id::from_v7_bytes([number; 16])
}

fn store_fixture(sql: &Connection) -> TestResult {
    let folder = path(r"C:\Fixture");
    let conditions = ScopeDraft {
        includes: vec![folder.clone()],
        exclusions: vec![],
        boundary: ScopeBoundary::CurrentVolume,
        included_mounts: vec![],
        source: ScopeSource::Gui,
    };
    let fingerprint = du_engine_api::scope::recent_scope_fingerprint_v1(&conditions)
        .map_err(|e| format!("{e:?}"))?;
    let body = json!({
        "bodyVersion":1,
        "normalizedScope":{"includes":[{"encoding":folder.encoding,"raw":folder.raw}],
            "exclusions":[],"boundary":"current_volume","includedMounts":[]},
        "targets":[{"appliedPath":folder,"volumeId":PathBytes::from_bytes(b"volume"),
            "entryPath":path(r"C:\"),"relativePath":path(r"\Fixture")}]
    });
    sql.execute(
        "INSERT INTO user_env_recent_scope VALUES(?1,?2,1,?3,1,30,30,30,1)",
        params![
            id(1).v7_bytes().ok_or("fixture id")?,
            fingerprint,
            body.to_string()
        ],
    )?;
    Ok(())
}

fn ipc(
    engine: &du_engine_api::api::Engine,
    reference: Id,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    let request = json!({"command":"du_call", "args":{"envelope":{
        "name":"scope.recent_open","contractVersion":1,"correlationId":id(9),
        "body":{"recentScopeRef":reference,"requestSeq":12}
    }}})
    .to_string();
    let response = crate::ipc::dispatch(engine, &request).ok_or("missing open response")?;
    Ok(serde_json::from_str(&response)?)
}

#[test]
fn missing_or_corrupt_row_does_not_recheck_any_target() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("scope.db");
    let db = AppDb::create_gui_with_history(&path).map_err(|e| format!("{e:?}"))?;
    let sql = Connection::open(&path)?;
    store_fixture(&sql)?;
    sql.execute("UPDATE user_env_recent_scope SET scope_body = '{}'", [])?;
    let mut builder = EngineBuilder::default();
    let volumes = Arc::new(Volumes {
        evidence: None,
        calls: AtomicUsize::new(0),
    });
    let roots = Arc::new(Roots::default());
    register(
        &mut builder,
        SharedDatabase::new(db, None),
        roots.clone(),
        volumes.clone(),
    );
    let engine = builder.build()?;
    assert_eq!(
        ipc(&engine, id(2))?
            .error
            .ok_or("missing not-found error")?
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        ipc(&engine, id(1))?
            .error
            .ok_or("missing corrupt-row error")?
            .code,
        ErrorCode::Internal
    );
    assert_eq!(volumes.calls.load(Ordering::Relaxed), 0);
    assert_eq!(roots.0.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn proposals_and_unverifiable_results_preserve_draft_and_never_admit_a_review() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path_on_disk = dir.path().join("scope.db");
    let db = AppDb::create_gui_with_history(&path_on_disk).map_err(|e| format!("{e:?}"))?;
    let sql = Connection::open(&path_on_disk)?;
    store_fixture(&sql)?;
    let database = SharedDatabase::new(db, None);
    let proposal = path(r"D:\");
    let complete = |entries| {
        Some(RecentVolumeObservation {
            identity_kind: b"fixture",
            entries,
        })
    };
    for (evidence, state, proposed, opens) in [
        (
            complete(vec![RecentVolumeEntry {
                path: proposal.clone(),
                same_entry: false,
            }]),
            "moved",
            json!(proposal),
            0,
        ),
        (complete(vec![]), "disconnected", Value::Null, 0),
        (None, "unverifiable", Value::Null, 0),
        (
            complete(vec![RecentVolumeEntry {
                path: path(r"C:\"),
                same_entry: true,
            }]),
            "matched",
            Value::Null,
            1,
        ),
    ] {
        let mut builder = EngineBuilder::default();
        let roots = Arc::new(Roots::default());
        register(
            &mut builder,
            database.clone(),
            roots.clone(),
            Arc::new(Volumes {
                evidence,
                calls: AtomicUsize::new(0),
            }),
        );
        let response = ipc(&builder.build()?, id(1))?;
        assert!(response.error.is_none());
        assert_eq!(response.body["draft"]["source"], "recent");
        assert_eq!(
            response.body["draft"]["includes"][0],
            json!(path(r"C:\Fixture"))
        );
        assert_eq!(response.body["targets"][0]["state"], state);
        assert_eq!(response.body["targets"][0]["proposedEntry"], proposed);
        assert!(response.body["review"].is_null());
        assert_eq!(roots.0.load(Ordering::Relaxed), opens);
    }
    let last_used_at: i64 = sql.query_row(
        "SELECT last_used_at_ms FROM user_env_recent_scope",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(last_used_at, 30);
    Ok(())
}
