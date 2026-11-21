//! GUI approval review, grant, revoke, and session state (F28).

mod commands;
mod execute;
mod results;
mod review;
mod state;

use review::{review_summary, validate_confirmations};

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex, Weak},
    time::{SystemTime, UNIX_EPOCH},
};

use du_change_executor::{
    ApprovalExecutionToken, ApprovalRecord, ApprovalRecordState, trash::ChangeResultHandle,
};
use du_core_contracts::{
    ByteSize, ENVELOPE_VERSION, EventEnvelope, Id, SizeAmount, SizeEvidence, Timestamp,
    approval_check::{ApprovalCheckFacts, ApprovalCheckRecorder},
    approval_ipc::{
        APPROVAL_CONFIRMATION_MISSING, APPROVAL_GRANT, APPROVAL_PLAN_SUPERSEDED,
        APPROVAL_PROTECTION_DENIED, APPROVAL_PROTECTION_UNDETERMINED, APPROVAL_REVIEW,
        APPROVAL_REVIEW_UNAVAILABLE, APPROVAL_REVOKE, APPROVAL_RULESET_CHANGED, APPROVAL_STALE,
        APPROVAL_STATE_CHANGED, APPROVAL_USER_REVOKED, ApprovalConfirmationProofs,
        ApprovalEventState, ApprovalGrantRequest, ApprovalGrantResponse, ApprovalOverwriteSummary,
        ApprovalProtectionResult, ApprovalProtectionSummary, ApprovalReviewRequest,
        ApprovalReviewResponse, ApprovalReviewSummary, ApprovalRevokeRequest,
        ApprovalRevokeResponse, ApprovalState, ApprovalStateChanged, RequestKind,
    },
    data_source::DataSource,
    plan_ipc::{PlanHash, PlanItemDisposition},
    plan_version_store::PlanVersionStore,
    platform_os_change::{ChangeOpsPort, SessionUndoStore},
    ports::Random,
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    desktop::{DesktopEvents, DesktopJobQueue},
    error::EngineError,
    plan::{
        ApprovalPrepareService, ApprovalPreparedPlan, ConfirmationToken, PlanItem, PlanItemState,
        PlanRejudgement, PlanVersion, PlannerAction as PlanAction, RejudgementState,
        RestoreRestriction,
    },
};
use serde_json::json;

use crate::{approval_source::check_request_plan_source, shared_database::SharedDatabase};

type VersionStore = Arc<Mutex<dyn PlanVersionStore + Send>>;
type CheckRecorder = Arc<Mutex<dyn ApprovalCheckRecorder + Send>>;

pub(super) struct ChangeHostConfig {
    pub(super) database: SharedDatabase,
    pub(super) events: Arc<DesktopEvents>,
    pub(super) random: Arc<dyn Random>,
    pub(super) primary_window: bool,
    pub(super) jobs: DesktopJobQueue,
    pub(super) ops: Arc<dyn ChangeOpsPort>,
    pub(super) undo: Arc<dyn SessionUndoStore>,
}

struct ChangeServices {
    jobs: DesktopJobQueue,
    ops: Arc<dyn ChangeOpsPort>,
    undo: Arc<dyn SessionUndoStore>,
    recorder: CheckRecorder,
    executions: Mutex<HashMap<Id, ChangeExecution>>,
    cursor_key: Mutex<[u8; 32]>,
}

struct ChangeExecution {
    request_version_id: Id,
    results: ChangeResultHandle,
    audit_facts: ApprovalCheckFacts,
    audit_finished: bool,
}

struct Entry {
    record: ApprovalRecord,
    token: ApprovalExecutionToken,
    state: ApprovalState,
}

struct Registry {
    entries: HashMap<Id, Entry>,
    states: HashMap<Id, ApprovalState>,
    session_id: Option<Id>,
    sequence: u64,
}

struct Runtime {
    prepare: Arc<dyn PlanApprovalPreparer>,
    versions: VersionStore,
    random: Arc<dyn Random>,
    events: Arc<DesktopEvents>,
    primary_window: bool,
    change: Option<Arc<ChangeServices>>,
    transition_lock: Mutex<()>,
    registry: Mutex<Registry>,
}

trait PlanApprovalPreparer: Send + Sync {
    fn prepare(&self, version_id: &Id) -> Result<ApprovalPreparedPlan, &'static str>;
}

impl PlanApprovalPreparer for ApprovalPrepareService {
    fn prepare(&self, version_id: &Id) -> Result<ApprovalPreparedPlan, &'static str> {
        ApprovalPrepareService::prepare(self, version_id)
    }
}

pub(super) fn register(
    builder: &mut EngineBuilder,
    prepare: ApprovalPrepareService,
    config: ChangeHostConfig,
) -> Result<(), du_engine_api::desktop::DesktopJobError> {
    config
        .jobs
        .register(du_change_executor::trash::descriptor())?;
    let versions: VersionStore = Arc::new(Mutex::new(config.database.clone()));
    let runtime = make_runtime_with_change(Arc::new(prepare), versions, config)?;
    builder.register(commands::Review(runtime.clone()));
    builder.register(commands::Grant(runtime.clone()));
    builder.register(commands::Revoke(runtime.clone()));
    builder.register(execute::ChangeExecute(runtime.clone()));
    builder.register(results::ChangeItemResults(runtime.clone()));
    let weak: Weak<Runtime> = Arc::downgrade(&runtime);
    runtime.events.subscribe(move |event| {
        let Some(runtime) = weak.upgrade() else {
            return;
        };
        runtime.observe_event(event);
    });
    let weak = Arc::downgrade(&runtime);
    builder.on_session_retire(move || {
        if let Some(runtime) = weak.upgrade() {
            runtime.retire();
        }
    });
    Ok(())
}

#[cfg(test)]
fn make_runtime(
    prepare: Arc<dyn PlanApprovalPreparer>,
    versions: VersionStore,
    random: Arc<dyn Random>,
    events: Arc<DesktopEvents>,
    primary_window: bool,
) -> Arc<Runtime> {
    let session_id = new_id(random.as_ref());
    Arc::new(Runtime {
        prepare,
        versions,
        random,
        events,
        primary_window,
        change: None,
        registry: Mutex::new(Registry {
            entries: HashMap::new(),
            states: HashMap::new(),
            session_id,
            sequence: 0,
        }),
        transition_lock: Mutex::new(()),
    })
}

fn make_runtime_with_change(
    prepare: Arc<dyn PlanApprovalPreparer>,
    versions: VersionStore,
    config: ChangeHostConfig,
) -> Result<Arc<Runtime>, du_engine_api::desktop::DesktopJobError> {
    let ChangeHostConfig {
        database,
        events,
        random,
        primary_window,
        jobs,
        ops,
        undo,
    } = config;
    let mut cursor_key = [0; 32];
    random
        .fill(&mut cursor_key)
        .map_err(|_| du_engine_api::desktop::DesktopJobError::InvalidState)?;
    let session_id = new_id(random.as_ref());
    let recorder: CheckRecorder = Arc::new(Mutex::new(database));
    let runtime = Arc::new(Runtime {
        prepare,
        versions,
        random,
        events,
        primary_window,
        change: Some(Arc::new(ChangeServices {
            jobs,
            ops,
            undo,
            recorder,
            executions: Mutex::new(HashMap::new()),
            cursor_key: Mutex::new(cursor_key),
        })),
        registry: Mutex::new(Registry {
            entries: HashMap::new(),
            states: HashMap::new(),
            session_id,
            sequence: 0,
        }),
        transition_lock: Mutex::new(()),
    });
    Ok(runtime)
}

fn verify_latest(store: &VersionStore, version: &PlanVersion) -> Result<bool, EngineError> {
    let records = store
        .lock()
        .map_err(|_| error("db.storage_unavailable"))?
        .read_lineage(&version.lineage_id)
        .map_err(|_| error("db.storage_unavailable"))?;
    Ok(records
        .last()
        .is_some_and(|record| record.plan_version_id == version.version_id))
}

fn new_id(random: &dyn Random) -> Option<Id> {
    let mut bytes = [0; 16];
    random.fill(&mut bytes).ok()?;
    Some(uuid_v7(bytes, now_ms()))
}

fn uuid_v7(mut bytes: [u8; 16], now: i64) -> Id {
    let millis = now.max(0) as u64;
    bytes[0] = (millis >> 40) as u8;
    bytes[1] = (millis >> 32) as u8;
    bytes[2] = (millis >> 24) as u8;
    bytes[3] = (millis >> 16) as u8;
    bytes[4] = (millis >> 8) as u8;
    bytes[5] = millis as u8;
    Id::from_v7_bytes(bytes)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_millis()).ok())
        .unwrap_or(0)
}

fn error(reason: &'static str) -> EngineError {
    EngineError::new(reason, Vec::new())
}

#[cfg(test)]
mod tests;
