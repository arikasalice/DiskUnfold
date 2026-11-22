//! F10 plan commands exercised through the GUI IPC dispatcher (FR1.HD2, FR10.HD1, FR11.HD1).
use super::*;
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id, ItemRef, OsPath, PathBytes, PathEncoding, ResultEnvelope,
    data_source::DataSource, result::ResultItemKind, volume::CapabilityState,
};
use du_engine_api::{
    plan::{
        AnalysisFactsPort, AnalysisItemFacts, DescendantResult, Fact, GuardResult,
        IdentityObservation, KnownLocation, LinkBoundary, PlanAnalysisSource,
        PlanPlatformFactsPort, PlanRulesetPort, PlanSources, PlanTargetCheckPort,
        PlatformProtectionFacts, ProtectionAction, ProtectionVolumeRole, RulesetSnapshot,
        TargetCheck, TargetIdentity,
    },
    size_rule,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PlanTestApp {
    engine: Engine,
    events: Arc<DesktopEvents>,
    profile: tempfile::TempDir,
    _database: crate::shared_database::SharedDatabase,
}

fn assemble(analysis: FakeAnalysis) -> TestResult<PlanTestApp> {
    let profile = tempfile::tempdir()?;
    let database = crate::startup_database::open(profile.path())?;
    database
        .0
        .connection
        .lock()
        .map_err(|_| "test database lock poisoned")?
        .append_ruleset(
            0,
            &du_app_db::ruleset::RulesetRow {
                version: 1,
                ruleset_hash: vec![7; 32],
                hash_format: "test.v1".into(),
                class_table_version: "1".into(),
                known_location_table_version: "1".into(),
                rule_format_version: 1,
                origin: "gui".into(),
                change_summary: "{}".into(),
                body: "{}".into(),
                created_at_ms: 1,
            },
        )
        .map_err(|error| format!("test ruleset setup failed: {error:?}"))?;
    let Some(ports) = scan_ports() else {
        return Err("no collection adapters on this platform".into());
    };
    let rules = super::size_rules::assemble(size_rule::default_size_rule_source())?;
    let versions: du_engine_api::plan::SharedVersionStore = Arc::new(Mutex::new(database.clone()));
    let sources = PlanSources::new(versions, Arc::new(database.clone()))
        .with_analysis(Arc::new(analysis))
        .with_fact_ports(Arc::new(MatchingTargets), Arc::new(FakePlatform))
        .with_ruleset_port(Arc::new(FakeRuleset));
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(du_engine_api::desktop::SharedTableLimit::default);
    let services = register_analysis_with_test_plan_sources(
        &mut builder,
        ports,
        &database,
        limit,
        rules,
        None,
        sources,
    )?;
    services.events.mark_document_ready();
    let events = services.events.clone();
    let engine = builder.build()?;
    Ok(PlanTestApp {
        engine,
        events,
        profile,
        _database: database,
    })
}

#[derive(Clone)]
struct FakeAnalysis {
    version_id: Id,
    data_source: DataSource,
    calls: Arc<AtomicUsize>,
    block_on_call: Option<usize>,
    entered: Option<mpsc::Sender<()>>,
    release: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
}

impl FakeAnalysis {
    fn real() -> Self {
        Self::new(DataSource::Real, None, None, None)
    }

    fn new(
        data_source: DataSource,
        block_on_call: Option<usize>,
        entered: Option<mpsc::Sender<()>>,
        release: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    ) -> Self {
        Self {
            version_id: Id::from_v7_bytes([1; 16]),
            data_source,
            calls: Arc::new(AtomicUsize::new(0)),
            block_on_call,
            entered,
            release,
        }
    }

    fn item_ref(&self, item_no: u32) -> ItemRef {
        ItemRef {
            result_version: self.version_id.clone(),
            item_no,
        }
    }
}

impl PlanAnalysisSource for FakeAnalysis {
    fn pin(
        &self,
        version: &Id,
    ) -> Result<Arc<dyn AnalysisFactsPort>, du_core_contracts::ports::PortError> {
        if version != &self.version_id {
            return Err(port_error("plan.result_stale"));
        }
        Ok(Arc::new(self.clone()))
    }
}

impl AnalysisFactsPort for FakeAnalysis {
    fn data_source(&self, version: &Id) -> Result<DataSource, du_core_contracts::ports::PortError> {
        (version == &self.version_id)
            .then_some(self.data_source)
            .ok_or_else(|| port_error("plan.result_stale"))
    }

    fn read_item(
        &self,
        item: &ItemRef,
    ) -> Result<AnalysisItemFacts, du_core_contracts::ports::PortError> {
        if item.result_version != self.version_id || item.item_no == 0 {
            return Err(port_error("plan.result_stale"));
        }
        let call = self.calls.fetch_add(1, Ordering::AcqRel) + 1;
        if self.block_on_call == Some(call) {
            if let Some(entered) = &self.entered {
                let _ = entered.send(());
            }
            if let Some(release) = &self.release {
                release
                    .lock()
                    .map_err(|_| port_error("support.internal_error"))?
                    .recv()
                    .map_err(|_| port_error("support.internal_error"))?;
            }
        }
        Ok(AnalysisItemFacts {
            path: OsPath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/fixture/file"),
                display: "/fixture/file".into(),
                lossy: false,
            },
            path_components: vec![b"/fixture".to_vec(), b"file".to_vec()],
            volume_id: b"fixture-volume".to_vec(),
            identity: Some(TargetIdentity {
                volume_id: b"fixture-volume".to_vec(),
                file_id: b"fixture-file".to_vec(),
            }),
            kind: ResultItemKind::File,
            size: None,
            modified_at: None,
            classification_id: Some("fixture.cleanable".into()),
        })
    }
}

struct MatchingTargets;
impl PlanTargetCheckPort for MatchingTargets {
    fn check(
        &self,
        _: &AnalysisItemFacts,
    ) -> Result<TargetCheck, du_core_contracts::ports::PortError> {
        Ok(TargetCheck::Matches)
    }
}

struct FakePlatform;
impl PlanPlatformFactsPort for FakePlatform {
    fn read(
        &self,
        _: &AnalysisItemFacts,
        _: &RulesetSnapshot,
    ) -> Result<PlatformProtectionFacts, du_core_contracts::ports::PortError> {
        Ok(PlatformProtectionFacts {
            identity: Fact::Known(IdentityObservation::Matches),
            link_boundary: Fact::Known(LinkBoundary::WithinVolume),
            volume_role: Fact::Known(ProtectionVolumeRole::Ordinary),
            known_location: Fact::Known(KnownLocation::NotListed),
            os_component: Fact::Known(false),
            age_guard: Fact::Known(GuardResult::Clear),
            permission_guard: Fact::Known(GuardResult::Clear),
            disk_backed_guard: Fact::Known(GuardResult::Clear),
            user_rule: Fact::Known(GuardResult::Clear),
            descendants: Fact::Known(DescendantResult::Clear),
            trash_capability: Some(CapabilityState::Supported),
        })
    }
}

struct FakeRuleset;
impl PlanRulesetPort for FakeRuleset {
    fn read_current(&self) -> Result<RulesetSnapshot, du_core_contracts::ports::PortError> {
        Ok(RulesetSnapshot {
            version: 1,
            hash: vec![7; 32],
            classification_version: 1,
            known_location_version: 1,
            class_actions: BTreeMap::from([(
                "fixture.cleanable".into(),
                ProtectionAction::Cleanable,
            )]),
        })
    }
}

#[test]
fn user_selection_build_commits_before_fixed_event_and_merges_duplicates() -> TestResult {
    let analysis = FakeAnalysis::real();
    let first_item = analysis.item_ref(1);
    let app = assemble(analysis)?;
    let opened = ok(
        &app.engine,
        "plan.draft_open",
        json!({"source":"manual","itemRefs":[first_item, first_item]}),
    )?;
    assert_eq!(opened["summary"]["itemCount"], 2);
    let build = ok(
        &app.engine,
        "plan.build",
        json!({"draftRef":opened["draftRef"],"editSeq":0}),
    )?;
    let fixed = wait_for_fixed(&app.events, &app.engine, &build["jobRef"])?;
    assert_eq!(fixed["jobRef"], build["jobRef"]);

    let absorbed = ok(
        &app.engine,
        "plan.get",
        json!({"planVersionId":fixed["planVersionId"],"section":"absorbed"}),
    )?;
    assert_eq!(absorbed["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        absorbed["items"][0]["reasonKey"],
        "plan.item.duplicate_target"
    );
    assert_eq!(
        absorbed["items"][0]["absorbedBy"],
        absorbed["items"][0]["itemRef"]
    );
    Ok(())
}

#[test]
fn cancel_during_plan_build_leaves_no_version_row() -> TestResult {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let analysis = FakeAnalysis::new(
        DataSource::Real,
        Some(1),
        Some(entered_tx),
        Some(Arc::new(Mutex::new(release_rx))),
    );
    let item = analysis.item_ref(1);
    let app = assemble(analysis)?;
    let opened = ok(
        &app.engine,
        "plan.draft_open",
        json!({"source":"manual","itemRefs":[item]}),
    )?;
    let build = ok(
        &app.engine,
        "plan.build",
        json!({"draftRef":opened["draftRef"],"editSeq":0}),
    )?;
    entered_rx.recv_timeout(Duration::from_secs(3))?;
    let cancelled = ok(
        &app.engine,
        "plan.draft_cancel",
        json!({"draftRef":opened["draftRef"]}),
    )?;
    assert_eq!(cancelled["discarded"], true);
    assert_eq!(cancelled["buildCancelled"], true);
    release_tx.send(())?;
    let terminal = wait_for_job(&app.engine, &build["jobRef"])?;
    assert_eq!(terminal["state"], "cancelled");

    let connection = rusqlite::Connection::open(
        app.profile
            .path()
            .join(du_platform_os::application_data::DATABASE_FILE),
    )?;
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM change_planner_plan_version",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
fn sample_and_snapshot_analysis_are_refused_at_gui_ipc_admission() -> TestResult {
    for data_source in [DataSource::Sample, DataSource::Snapshot] {
        let analysis = FakeAnalysis::new(data_source, None, None, None);
        let item = analysis.item_ref(1);
        let app = assemble(analysis)?;
        let result = call(
            &app.engine,
            "plan.draft_open",
            json!({"source":"manual","itemRefs":[item]}),
        )?;
        assert_eq!(
            result.error.as_ref().map(|error| error.reason_key.as_str()),
            Some("plan.source_not_current"),
            "{data_source:?}"
        );
    }
    Ok(())
}

#[test]
fn unimplemented_source_conversions_are_denied_without_placeholder_behavior() -> TestResult {
    let app = assemble(FakeAnalysis::real())?;
    for source in ["goal_alternative", "duplicate_keep", "scenario"] {
        let result = call(
            &app.engine,
            "plan.draft_open",
            json!({"source":source,"itemRefs":[]}),
        )?;
        assert_eq!(
            result.error.as_ref().map(|error| error.reason_key.as_str()),
            Some("plan.source_channel_denied"),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn evicted_draft_returns_the_session_evicted_reason() -> TestResult {
    let analysis = FakeAnalysis::real();
    let item = analysis.item_ref(1);
    let app = assemble(analysis)?;
    let mut first = Value::Null;
    for index in 0..33 {
        let opened = ok(
            &app.engine,
            "plan.draft_open",
            json!({"source":"manual","itemRefs":[item]}),
        )?;
        if index == 0 {
            first = opened;
        }
    }
    let stale = call(
        &app.engine,
        "plan.draft_edit",
        json!({
            "draftRef":first["draftRef"],
            "editSeq":0,
            "edit":{"kind":"set_overwrite","itemRef":item,"overwrite":true}
        }),
    )?;
    assert_eq!(
        stale.error.as_ref().map(|error| error.reason_key.as_str()),
        Some("session.evicted")
    );
    Ok(())
}

fn wait_for_fixed(events: &DesktopEvents, engine: &Engine, job_ref: &Value) -> TestResult<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(json) = events.next_json() {
            let event: Value = serde_json::from_str(&json)?;
            if event["topic"] == "plan.version_fixed" {
                let result = ok(
                    engine,
                    "plan.get",
                    json!({"planVersionId":event["body"]["planVersionId"],"section":"executable"}),
                )?;
                assert!(result["items"].is_array());
                return Ok(event["body"].clone());
            }
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let terminal = wait_for_job(engine, job_ref).ok();
    Err(format!("plan.version_fixed not delivered for job {job_ref}; terminal={terminal:?}").into())
}

fn wait_for_job(engine: &Engine, job_ref: &Value) -> TestResult<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let status = ok(engine, "job.get", json!({"jobRef":job_ref}))?;
        if matches!(
            status["state"].as_str(),
            Some("done" | "partial" | "failed" | "cancelled")
        ) {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err("plan.build did not reach a terminal state".into())
}

fn ok(engine: &Engine, name: &str, body: Value) -> TestResult<Value> {
    let response = call(engine, name, body)?;
    if let Some(error) = response.error {
        return Err(format!("{name}: {}", error.reason_key).into());
    }
    Ok(response.body)
}

fn call(engine: &Engine, name: &str, body: Value) -> TestResult<ResultEnvelope> {
    let correlation_id = Id::from_v7_bytes([42; 16]);
    let envelope = CallEnvelope {
        name: name.into(),
        contract_version: ENVELOPE_VERSION,
        correlation_id: correlation_id.clone(),
        body,
    };
    let message = json!({"command":"du_call","args":{"envelope":envelope}});
    let response = crate::ipc::dispatch(engine, &message.to_string()).ok_or("no IPC response")?;
    Ok(serde_json::from_str(&response)?)
}

fn port_error(reason_key: &str) -> du_core_contracts::ports::PortError {
    du_core_contracts::ports::PortError {
        kind: du_core_contracts::ports::PortErrorKind::NotFound,
        reason_key: reason_key.into(),
        os_code: None,
    }
}
