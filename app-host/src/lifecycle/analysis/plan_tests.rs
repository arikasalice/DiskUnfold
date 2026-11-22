//! F10 plan commands exercised through the GUI IPC dispatcher (FR1.HD2, FR10.HD1, FR11.HD1).
use super::*;
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id, ResultEnvelope,
    data_source::DataSource,
    platform_os_change::{ChangeOpsPort, SessionUndoStore},
    volume::CapabilityState,
};
use du_engine_api::{
    plan::{
        AnalysisItemFacts, DescendantResult, Fact, GuardResult, IdentityObservation, KnownLocation,
        LinkBoundary, PlanPlatformFactsPort, PlanRulesetPort, PlanSources, PlanTargetCheckPort,
        PlatformProtectionFacts, ProtectionAction, ProtectionVolumeRole, RulesetSnapshot,
        TargetCheck,
    },
    size_rule,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) struct PlanTestApp {
    pub(super) engine: Engine,
    pub(super) events: Arc<DesktopEvents>,
    profile: tempfile::TempDir,
    _database: crate::shared_database::SharedDatabase,
    undo: Arc<dyn SessionUndoStore>,
}

fn assemble(analysis: FakeAnalysis) -> TestResult<PlanTestApp> {
    assemble_with_protection_facts(analysis, None)
}

pub(super) fn assemble_with_protection_facts(
    analysis: FakeAnalysis,
    protection_facts: Option<Arc<dyn du_core_contracts::protection_facts::ProtectionFactsPort>>,
) -> TestResult<PlanTestApp> {
    assemble_with_ports(
        analysis,
        protection_facts,
        Arc::new(du_platform_os::change_ops::SystemChangeOps),
        Arc::new(crate::session_undo_info::SessionUndoInfoRegistry::default()),
    )
}

fn assemble_with_ports(
    analysis: FakeAnalysis,
    protection_facts: Option<Arc<dyn du_core_contracts::protection_facts::ProtectionFactsPort>>,
    change_ops: Arc<dyn ChangeOpsPort>,
    undo: Arc<dyn SessionUndoStore>,
) -> TestResult<PlanTestApp> {
    let profile = tempfile::tempdir()?;
    let database = crate::startup_database::open(profile.path())?;
    let ruleset = du_engine_api::plan::built_in_protection_ruleset_row(1)
        .map_err(|error| format!("test ruleset build failed: {error:?}"))?;
    database
        .0
        .connection
        .lock()
        .map_err(|_| "test database lock poisoned")?
        .initialize_builtin_ruleset_if_empty(&ruleset)
        .map_err(|error| format!("test ruleset setup failed: {error:?}"))?;
    let Some(ports) = scan_ports() else {
        return Err("no collection adapters on this platform".into());
    };
    let rules = super::size_rules::assemble(size_rule::default_size_rule_source())?;
    let versions: du_engine_api::plan::SharedVersionStore = Arc::new(Mutex::new(database.clone()));
    let mut sources = PlanSources::new(versions, Arc::new(database.clone()))
        .with_analysis(Arc::new(analysis))
        .with_fact_ports(Arc::new(MatchingTargets), Arc::new(FakePlatform));
    if let Some(facts) = protection_facts {
        sources = sources.with_protection_facts(facts);
    } else {
        sources = sources.with_ruleset_port(Arc::new(FakeRuleset));
    }
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(du_engine_api::desktop::SharedTableLimit::default);
    let services = register_analysis_with_test_plan_sources(
        &mut builder,
        ports,
        &database,
        limit,
        rules,
        None,
        super::PlanChangeServices {
            plan_sources: Some(sources),
            undo: Some(undo.clone()),
            ops: change_ops,
        },
    )?;
    // The history reader module is Windows-only, so the history.items command is registered
    // only there; the per-item rows are asserted from SQLite on every platform.
    #[cfg(windows)]
    {
        let history_reader = crate::history::HistoryReader::open(profile.path(), database.clone())?;
        du_engine_api::history::items::register(
            &mut builder,
            history_reader,
            &du_platform_os::entropy::OsRandom,
        )
        .map_err(|error| format!("history items registration failed: {error:?}"))?;
    }
    services.events.mark_document_ready();
    let events = services.events.clone();
    let engine = builder.build()?;
    Ok(PlanTestApp {
        engine,
        events,
        profile,
        _database: database,
        undo,
    })
}

mod source;
pub(super) use source::FakeAnalysis;

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
        _: du_engine_api::plan::PlannerAction,
    ) -> Result<PlatformProtectionFacts, du_core_contracts::ports::PortError> {
        Ok(PlatformProtectionFacts {
            identity: Fact::Known(IdentityObservation::Matches),
            link_boundary: Fact::Known(LinkBoundary::WithinVolume),
            volume_role: Fact::Known(ProtectionVolumeRole::Ordinary),
            known_location: Fact::Known(KnownLocation::NotListed),
            classification_id: Some("fixture.cleanable".into()),
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
            hash_format: "du.protect.ruleset.v1".into(),
            classification_version: 1,
            known_location_version: 1,
            rule_format_version: 1,
            user_rules: Vec::new(),
            class_actions: BTreeMap::from([("fixture.cleanable".into(), ProtectionAction::Allow)]),
            class_release_state: BTreeMap::from([(
                "fixture.cleanable".into(),
                du_engine_api::plan::ClassReleaseState::NotReleasable,
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

mod trash_tests;

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

pub(super) fn wait_for_fixed(
    events: &DesktopEvents,
    engine: &Engine,
    job_ref: &Value,
) -> TestResult<Value> {
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

pub(super) fn ok(engine: &Engine, name: &str, body: Value) -> TestResult<Value> {
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
