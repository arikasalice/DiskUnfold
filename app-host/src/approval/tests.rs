use super::*;
use du_core_contracts::{
    ByteSize, CallEnvelope, ENVELOPE_VERSION, Id, ItemRef, OsPath, PathBytes, PathEncoding,
    ResultEnvelope, SizeAmount, SizeBasis, SizeValue, Timestamp,
    approval_ipc::{ApprovalConfirmationProofs, ApprovalReviewRequest, ApprovalRevokeRequest},
    plan_version_store::{PlanOriginChannel, PlanReadiness, PlanVersionRecord},
    ports::{PortError, PortErrorKind},
    result::ResultItemKind,
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder},
    plan::{
        ApprovalPreparedPlan, DomainPlanReadiness, PlannerProtectionSummary,
        PlannerRequestChannel as RequestChannel, PlannerRulesetVersion as RulesetVersion,
        PlannerSourceKind as PlanSourceKind, ProtectionVerdict,
    },
};
use serde_json::{Value, json};
use std::{
    sync::atomic::{AtomicU8, Ordering},
    time::Duration,
};

struct MemoryPlans(Vec<PlanVersionRecord>);

impl PlanVersionStore for MemoryPlans {
    fn append_fixed(&mut self, value: &PlanVersionRecord) -> Result<(), PortError> {
        self.0.push(value.clone());
        Ok(())
    }
    fn read(&self, id: &Id) -> Result<PlanVersionRecord, PortError> {
        self.0
            .iter()
            .find(|record| &record.plan_version_id == id)
            .cloned()
            .ok_or_else(storage_missing)
    }
    fn read_lineage(&self, id: &Id) -> Result<Vec<PlanVersionRecord>, PortError> {
        Ok(self
            .0
            .iter()
            .filter(|record| &record.lineage_id == id)
            .cloned()
            .collect())
    }
}

struct FakePreparer(ApprovalPreparedPlan);
impl PlanApprovalPreparer for FakePreparer {
    fn prepare(&self, id: &Id) -> Result<ApprovalPreparedPlan, &'static str> {
        (id == &self.0.version.version_id)
            .then(|| ApprovalPreparedPlan {
                version: self.0.version.clone(),
                rejudgement: self.0.rejudgement.clone(),
            })
            .ok_or("approval.request_not_found")
    }
}

struct FixedRandom(AtomicU8);
impl Random for FixedRandom {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        let value = self.0.fetch_add(1, Ordering::Relaxed);
        bytes.fill(value);
        Ok(())
    }
}

type ApprovalHarness = (du_engine_api::api::Engine, Arc<Runtime>, Arc<DesktopEvents>);

fn id(seed: u8) -> Id {
    let mut bytes = [seed; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    Id::from_v7_bytes(bytes)
}

fn plan(
    request_id: Id,
    lineage: Id,
    action: PlanAction,
    data_source: DataSource,
) -> Result<PlanVersion, Box<dyn std::error::Error>> {
    let item_ref = ItemRef {
        result_version: id(7),
        item_no: 1,
    };
    let item = PlanItem {
        item_no: 1,
        item: item_ref,
        path: OsPath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/tmp/review-target"),
            display: "/tmp/review-target".into(),
            lossy: false,
        },
        path_components: vec![b"tmp".to_vec(), b"review-target".to_vec()],
        volume_id: b"test-volume".to_vec(),
        expected_identity: None,
        expected_kind: ResultItemKind::File,
        expected_size: Some(SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount: SizeAmount::Exact {
                bytes: ByteSize::from(12),
            },
        }),
        expected_modified_at: None,
        classification_id: Some("class.user".into()),
        action,
        destination: None,
        overwrite: false,
        displaced_size: None,
        condition: Vec::new(),
        selection_evidence: vec!["user_selection".into()],
        state: PlanItemState::Included,
        reason_key: None,
        absorbed_by: None,
        protection: Some(PlannerProtectionSummary {
            verdict: ProtectionVerdict::Allow,
            classification_id: Some("class.user".into()),
            released: false,
            evaluators: Vec::new(),
            evidence: Vec::new(),
        }),
        restore: RestoreRestriction::Supported,
    };
    Ok(PlanVersion {
        version_id: request_id,
        lineage_id: lineage,
        parent_version_id: None,
        source_version_id: id(7),
        source_kind: PlanSourceKind::UserSelection,
        data_source,
        request_channel: RequestChannel::Gui,
        ruleset: Some(RulesetVersion {
            version: 1,
            hash: vec![1; 32],
            classification_version: 1,
            known_location_version: 1,
        }),
        items: vec![item],
        expected_effect: Vec::new(),
        confirmations: Vec::new(),
        readiness: DomainPlanReadiness::Ready,
        hash_format: "du.plan.hash.v1".into(),
        hash: [9; 32],
        created_at: Timestamp::from_unix_millis(1)
            .ok_or_else(|| std::io::Error::other("invalid timestamp"))?,
    })
}

fn prepared(version: PlanVersion) -> ApprovalPreparedPlan {
    ApprovalPreparedPlan {
        rejudgement: PlanRejudgement {
            version_id: version.version_id.clone(),
            recomputed_hash: version.hash,
            ruleset_version: Some(1),
            items: vec![(1, RejudgementState::Same)],
        },
        version,
    }
}

fn record(version: &PlanVersion, created: i64) -> PlanVersionRecord {
    PlanVersionRecord {
        plan_version_id: version.version_id.clone(),
        lineage_id: version.lineage_id.clone(),
        parent_plan_version_id: version.parent_version_id.clone(),
        version_hash: version.hash,
        hash_format: version.hash_format.clone(),
        ruleset_version: 1,
        readiness: PlanReadiness::Ready,
        origin_channel: PlanOriginChannel::Gui,
        body: "{}".into(),
        created_at_ms: created,
        data_source: version.data_source,
    }
}

fn make_engine(
    version: PlanVersion,
    versions: MemoryPlans,
    primary: bool,
) -> Result<ApprovalHarness, Box<dyn std::error::Error>> {
    let shared: VersionStore = Arc::new(Mutex::new(versions));
    let events = Arc::new(DesktopEvents::default());
    let runtime = make_runtime(
        Arc::new(FakePreparer(prepared(version))),
        shared,
        Arc::new(FixedRandom(AtomicU8::new(1))),
        events.clone(),
        primary,
    );
    let mut builder = EngineBuilder::default();
    builder.register(Review(runtime.clone()));
    builder.register(Grant(runtime.clone()));
    builder.register(Revoke(runtime.clone()));
    Ok((builder.build()?, runtime, events))
}

fn call(
    engine: &du_engine_api::api::Engine,
    name: &str,
    body: Value,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    let correlation_id = id(90);
    let envelope = CallEnvelope {
        name: name.into(),
        contract_version: ENVELOPE_VERSION,
        correlation_id,
        body,
    };
    let request = json!({"command":"du_call", "args":{"envelope":envelope}});
    let response = crate::ipc::dispatch(engine, &request.to_string())
        .ok_or_else(|| std::io::Error::other("missing IPC response"))?;
    Ok(serde_json::from_str(&response)?)
}

#[test]
fn review_reads_a_stored_plan_and_sample_is_denied_at_the_ipc_entry()
-> Result<(), Box<dyn std::error::Error>> {
    let sample = plan(id(1), id(2), PlanAction::Trash, DataSource::Sample)?;
    let stored = record(&sample, 1);
    let (engine, _, _) = make_engine(sample.clone(), MemoryPlans(vec![stored]), true)?;
    let response = call(
        &engine,
        APPROVAL_REVIEW,
        serde_json::to_value(ApprovalReviewRequest {
            request_version_id: sample.version_id.clone(),
            request_kind: RequestKind::Plan,
        })?,
    )?;
    assert_eq!(
        response.error.map(|e| e.reason_key).as_deref(),
        Some("approval.sample_source_denied")
    );

    let real = plan(id(3), id(4), PlanAction::Trash, DataSource::Real)?;
    let (engine, _, _) = make_engine(real.clone(), MemoryPlans(vec![record(&real, 1)]), true)?;
    let response = call(
        &engine,
        APPROVAL_REVIEW,
        serde_json::to_value(ApprovalReviewRequest {
            request_version_id: real.version_id,
            request_kind: RequestKind::Plan,
        })?,
    )?;
    assert!(response.error.is_none());
    assert_eq!(response.body["summary"]["targetCount"], 1);

    let correlation = id(91);
    let mcp = engine.call(
        CallEnvelope {
            name: APPROVAL_REVIEW.into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: correlation.clone(),
            body: serde_json::to_value(ApprovalReviewRequest {
                request_version_id: id(3),
                request_kind: RequestKind::Plan,
            })?,
        },
        &CallerCtx::new(Channel::Mcp, correlation.clone(), None),
    );
    assert_eq!(
        mcp.error.map(|e| e.reason_key).as_deref(),
        Some("command.not_provided")
    );
    for name in [
        APPROVAL_GRANT,
        APPROVAL_REVOKE,
        "approval.prepare",
        "change.execute",
    ] {
        let route = engine.call(
            CallEnvelope {
                name: name.into(),
                contract_version: ENVELOPE_VERSION,
                correlation_id: correlation.clone(),
                body: json!({"requestVersionId":id(3),"presentedPlanHash":"0".repeat(64)}),
            },
            &CallerCtx::new(Channel::Mcp, correlation.clone(), None),
        );
        assert_eq!(
            route.error.map(|e| e.reason_key).as_deref(),
            Some("command.not_provided"),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn snapshot_plans_are_denied_before_review_or_grant_checks()
-> Result<(), Box<dyn std::error::Error>> {
    let snapshot = plan(id(92), id(93), PlanAction::Trash, DataSource::Snapshot)?;
    let (engine, _, _) = make_engine(
        snapshot.clone(),
        MemoryPlans(vec![record(&snapshot, 1)]),
        false,
    )?;

    let reviewed = call(
        &engine,
        APPROVAL_REVIEW,
        serde_json::to_value(ApprovalReviewRequest {
            request_version_id: snapshot.version_id.clone(),
            request_kind: RequestKind::Plan,
        })?,
    )?;
    assert_eq!(
        reviewed.error.map(|error| error.reason_key).as_deref(),
        Some("plan.source_not_current")
    );

    let granted = call(
        &engine,
        APPROVAL_GRANT,
        json!({
            "requestVersionId": snapshot.version_id,
            "presentedPlanHash": "0".repeat(64)
        }),
    )?;
    assert_eq!(
        granted.error.map(|error| error.reason_key).as_deref(),
        Some("plan.source_not_current")
    );
    Ok(())
}

#[test]
fn delete_grant_requires_proofs_and_revoke_emits_after_state_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let version = plan(id(5), id(6), PlanAction::Delete, DataSource::Real)?;
    let (engine, runtime, events) = make_engine(
        version.clone(),
        MemoryPlans(vec![record(&version, 1)]),
        true,
    )?;
    events.mark_document_ready();
    let review = call(
        &engine,
        APPROVAL_REVIEW,
        serde_json::to_value(ApprovalReviewRequest {
            request_version_id: version.version_id.clone(),
            request_kind: RequestKind::Plan,
        })?,
    )?;
    assert!(review.error.is_none());
    let body = json!({"requestVersionId":version.version_id,"presentedPlanHash":review.body["planHash"],"confirmations":null});
    let missing = call(&engine, APPROVAL_GRANT, body)?;
    assert_eq!(
        missing.error.map(|e| e.reason_key).as_deref(),
        Some("approval.confirmation_missing")
    );

    let committed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let weak = Arc::downgrade(&runtime);
    events.subscribe({
        let committed = committed.clone();
        move |event| {
            if event.topic == APPROVAL_STATE_CHANGED {
                let state = weak.upgrade().and_then(|runtime| {
                    runtime
                        .registry
                        .lock()
                        .ok()
                        .map(|registry| registry.states.get(&id(5)).copied())
                });
                committed.store(
                    state.flatten() == Some(ApprovalState::Granted),
                    Ordering::Release,
                );
            }
        }
    });
    let proofs = ApprovalConfirmationProofs {
        permanent_delete_selected: Some(true),
        confirmed_target_count: Some(1),
        confirmed_target_size: Some(ByteSize::from(12)),
        irreversibility_confirmed: Some(true),
        basic_protection_unlock_confirmed: None,
        overwrite_confirmed: None,
        confirmed_overwrite_item_count: None,
        confirmed_displaced_bytes: None,
    };
    let granted = call(
        &engine,
        APPROVAL_GRANT,
        json!({"requestVersionId":version.version_id,"presentedPlanHash":review.body["planHash"],"confirmations":proofs}),
    )?;
    assert!(granted.error.is_none());
    assert!(committed.load(Ordering::Acquire));
    let revoked = call(
        &engine,
        APPROVAL_REVOKE,
        serde_json::to_value(ApprovalRevokeRequest {
            request_version_id: version.version_id.clone(),
        })?,
    )?;
    assert!(revoked.error.is_none());
    assert_eq!(revoked.body["state"], "withdrawn");
    std::thread::sleep(Duration::from_millis(110));
    let event = events.next_json().ok_or("missing approval state event")?;
    assert!(event.contains("approval.state_changed"));
    Ok(())
}

#[test]
fn a_fixed_successor_invalidates_the_previous_grant() -> Result<(), Box<dyn std::error::Error>> {
    let old = plan(id(10), id(11), PlanAction::Trash, DataSource::Real)?;
    let new = plan(
        id(12),
        old.lineage_id.clone(),
        PlanAction::Trash,
        DataSource::Real,
    )?;
    let (engine, runtime, events) =
        make_engine(old.clone(), MemoryPlans(vec![record(&old, 1)]), true)?;
    events.mark_document_ready();
    let review = call(
        &engine,
        APPROVAL_REVIEW,
        serde_json::to_value(ApprovalReviewRequest {
            request_version_id: old.version_id.clone(),
            request_kind: RequestKind::Plan,
        })?,
    )?;
    let granted = call(
        &engine,
        APPROVAL_GRANT,
        json!({"requestVersionId":old.version_id,"presentedPlanHash":review.body["planHash"]}),
    )?;
    assert!(granted.error.is_none());

    runtime
        .versions
        .lock()
        .map_err(|_| "lock")?
        .append_fixed(&record(&new, 2))
        .map_err(|_| std::io::Error::other("append fixed plan"))?;
    runtime.observe_event(EventEnvelope {
        topic: "plan.version_fixed".into(),
        contract_version: ENVELOPE_VERSION,
        seq: 0,
        correlation_id: None,
        job_ref: None,
        result_version: None,
        occurred_at: Timestamp::from_unix_millis(2)
            .ok_or_else(|| std::io::Error::other("timestamp"))?,
        body: json!({"replacedPlanVersionId":old.version_id}),
    });
    assert_eq!(
        runtime
            .registry
            .lock()
            .map_err(|_| "registry")?
            .states
            .get(&old.version_id),
        Some(&ApprovalState::Invalidated)
    );
    let state = call(
        &engine,
        APPROVAL_REVIEW,
        serde_json::to_value(ApprovalReviewRequest {
            request_version_id: old.version_id.clone(),
            request_kind: RequestKind::Plan,
        })?,
    )?;
    assert_eq!(state.body["currentApprovalState"], "invalidated");
    let stale_grant = call(
        &engine,
        APPROVAL_GRANT,
        json!({"requestVersionId":old.version_id,"presentedPlanHash":review.body["planHash"]}),
    )?;
    assert_eq!(
        stale_grant.error.map(|e| e.reason_key).as_deref(),
        Some("approval.plan_superseded")
    );
    std::thread::sleep(Duration::from_millis(110));
    assert!(events.next_json().is_some());
    Ok(())
}

fn storage_missing() -> PortError {
    PortError {
        kind: PortErrorKind::NotFound,
        reason_key: "db.not_found".into(),
        os_code: None,
    }
}
