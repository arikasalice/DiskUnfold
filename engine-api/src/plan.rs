//! GUI plan drafts, immutable plan reads, and the `plan.build` job (F10).
mod analysis;
mod approval_prepare;
mod job;
mod protection_facts;
mod sources;
mod storage;
mod usecases;

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak},
};

use du_change_planner::ports::{
    AnalysisFactsPort as DomainAnalysisFactsPort, ClockPort, PlanAction, PlanIdPort, PlanSelection,
    PlanVersionPort, PlatformFactsPort as DomainPlatformFactsPort, ProtectionRulesetPort,
    TargetCheck as DomainTargetCheck, TargetCheckPort,
};
use du_core_contracts::{
    Id, Timestamp,
    plan_ipc::{
        PLAN_EMPTY, PLAN_RESULT_STALE, PLAN_SOURCE_CHANNEL_DENIED, PLAN_SOURCE_NOT_CURRENT,
        PLAN_TOO_MANY_ITEMS, PlanDraftCancelRequest, PlanDraftCancelResponse, PlanDraftEdit,
        PlanDraftEditRequest, PlanDraftEditResponse, PlanDraftOpenRequest, PlanDraftOpenResponse,
        PlanDraftSummary, PlanSourceCount, PlanSourceKind, SESSION_EVICTED,
    },
};
use du_job_runtime::{JobClock, JobIdentity, JobRuntime};

use crate::{api::EngineBuilder, desktop::DesktopEvents, error::EngineError, workers::Workers};

pub use analysis::{IngestPlanAnalysisSource, PlanAnalysisSource};
pub use approval_prepare::{ApprovalPrepareService, ApprovalPreparedPlan};
pub use du_change_planner::ports::{
    AnalysisFactsPort, AnalysisItemFacts, ClassReleaseState, ConfirmationToken, DescendantResult,
    Fact, GuardResult, IdentityObservation, KnownLocation, KnownLocationKind, LinkBoundary,
    PlanAction as PlannerAction, PlanItem, PlanItemState, PlanRejudgement,
    PlanSourceKind as PlannerSourceKind, PlanVersion, PlatformFactsPort as PlanPlatformFactsPort,
    PlatformProtectionFacts, ProtectionAction, ProtectionRulesetPort as PlanRulesetPort,
    ProtectionSummary as PlannerProtectionSummary, ProtectionVerdict, ProtectionVolumeRole,
    RejudgementState, RequestChannel as PlannerRequestChannel, RestoreRestriction, RulesetSnapshot,
    RulesetVersion as PlannerRulesetVersion, TargetCheck, TargetCheckPort as PlanTargetCheckPort,
    TargetIdentity,
};
pub use du_change_planner::readiness::PlanReadiness as DomainPlanReadiness;
pub use sources::{PlanSources, built_in_protection_ruleset_row};
use storage::VersionReader;
pub use storage::{RulesetAdapter, SharedVersionStore};
use usecases::{UnknownPlatformFacts, UnknownTargetCheck};

const MAX_DRAFTS: usize = 32;

/// Job identity and clock supplied by the desktop runtime.
pub(crate) struct PlanJobPorts {
    identity: Arc<dyn JobIdentity>,
    clock: Arc<dyn JobClock>,
}

impl PlanJobPorts {
    /// Combines the desktop's job identity and clock for plan registration.
    pub(crate) fn new(identity: Arc<dyn JobIdentity>, clock: Arc<dyn JobClock>) -> Self {
        Self { identity, clock }
    }
}

#[derive(Default)]
struct Drafts {
    entries: HashMap<Id, Draft>,
    order: VecDeque<Id>,
    evicted: VecDeque<Id>,
}

struct Draft {
    source_version_id: Id,
    analysis: Arc<dyn DomainAnalysisFactsPort>,
    selections: Vec<PlanSelection>,
    edit_seq: u64,
    last_build: Option<BuildReceipt>,
    last_fixed: Option<Id>,
    operation: Arc<Mutex<()>>,
}

#[derive(Clone)]
struct BuildReceipt {
    edit_seq: u64,
    job_ref: Id,
}

pub(super) struct PlanService {
    drafts: Mutex<Drafts>,
    analysis_source: Arc<dyn PlanAnalysisSource>,
    target_check: Arc<dyn TargetCheckPort>,
    platform: Arc<dyn DomainPlatformFactsPort>,
    rulesets: Arc<dyn ProtectionRulesetPort>,
    versions: storage::SharedVersionStore,
    version_reader: Arc<dyn PlanVersionPort>,
    ids: Arc<dyn PlanIdPort>,
    clock: Arc<dyn ClockPort>,
    runtime: Arc<JobRuntime>,
    workers: Arc<Workers>,
    events: Arc<DesktopEvents>,
}

/// Registers the seven GUI plan commands and the `plan.build` descriptor.
pub(crate) fn register(
    builder: &mut EngineBuilder,
    runtime: Arc<JobRuntime>,
    workers: Arc<Workers>,
    events: Arc<DesktopEvents>,
    analysis_index: du_analysis_index::IngestSink,
    job_ports: PlanJobPorts,
    sources: PlanSources,
) -> Result<ApprovalPrepareService, du_job_runtime::RuntimeError> {
    runtime.register(job::descriptor())?;
    let analysis_source = sources
        .analysis
        .unwrap_or_else(|| Arc::new(IngestPlanAnalysisSource::new(analysis_index)));
    let target_check = sources
        .target_check
        .unwrap_or_else(|| Arc::new(UnknownTargetCheck));
    let platform = sources
        .platform
        .unwrap_or_else(|| Arc::new(UnknownPlatformFacts));
    let service = Arc::new(PlanService {
        drafts: Mutex::new(Drafts::default()),
        analysis_source,
        target_check,
        platform,
        rulesets: sources.rulesets,
        version_reader: Arc::new(VersionReader(sources.versions.clone())),
        versions: sources.versions,
        ids: Arc::new(PlanIds(job_ports.identity)),
        clock: Arc::new(PlanTime(job_ports.clock)),
        runtime,
        workers,
        events,
    });
    usecases::register(builder, service.clone());
    let retired: Weak<PlanService> = Arc::downgrade(&service);
    builder.on_session_retire(move || {
        if let Some(service) = retired.upgrade() {
            service.retire();
        }
    });
    Ok(service.approval_prepare_service())
}

impl PlanService {
    fn open(&self, request: PlanDraftOpenRequest) -> Result<PlanDraftOpenResponse, &'static str> {
        if request.source != PlanSourceKind::Manual {
            return Err(PLAN_SOURCE_CHANNEL_DENIED);
        }
        if request.item_refs.is_empty() {
            return Err(PLAN_EMPTY);
        }
        if request.item_refs.len() > du_change_planner::api::MAX_PLAN_ITEMS {
            return Err(PLAN_TOO_MANY_ITEMS);
        }
        if request.alternative_ref.is_some() {
            return Err(PLAN_SOURCE_NOT_CURRENT);
        }
        let version_id = request.item_refs[0].result_version.clone();
        if request
            .item_refs
            .iter()
            .any(|item| item.result_version != version_id)
        {
            return Err(PLAN_SOURCE_NOT_CURRENT);
        }
        let analysis = self
            .analysis_source
            .pin(&version_id)
            .map_err(|_| PLAN_RESULT_STALE)?;
        let data_source = analysis
            .data_source(&version_id)
            .map_err(|_| PLAN_RESULT_STALE)?;
        if data_source != du_core_contracts::data_source::DataSource::Real {
            return Err(PLAN_SOURCE_NOT_CURRENT);
        }
        let mut selections = Vec::with_capacity(request.item_refs.len());
        for item in request.item_refs {
            selections.push(PlanSelection {
                item,
                action: PlanAction::Trash,
                condition: Vec::new(),
                selection_evidence: vec!["user_selection".into()],
                destination: None,
                overwrite: false,
                displaced_size: None,
            });
        }
        let item_count = selections.len();
        let draft_ref = self.ids.next_id().map_err(|_| "support.internal_error")?;
        let mut drafts = self.drafts.lock().map_err(|_| "support.internal_error")?;
        let mut evicted = Vec::new();
        while drafts.entries.len() >= MAX_DRAFTS {
            if let Some(oldest) = drafts.order.pop_front()
                && let Some(draft) = drafts.entries.remove(&oldest)
            {
                if let Some(build) = draft.last_build {
                    evicted.push(build.job_ref);
                }
                remember_evicted(&mut drafts, oldest);
            }
        }
        drafts.entries.insert(
            draft_ref.clone(),
            Draft {
                source_version_id: version_id,
                analysis,
                selections,
                edit_seq: 0,
                last_build: None,
                last_fixed: None,
                operation: Arc::new(Mutex::new(())),
            },
        );
        touch(&mut drafts, &draft_ref);
        let response = PlanDraftOpenResponse {
            draft_ref,
            edit_seq: 0,
            summary: summary(item_count),
        };
        drop(drafts);
        for job_ref in evicted {
            let _ = self.runtime.cancel(&job_ref);
        }
        Ok(response)
    }

    fn edit(&self, request: PlanDraftEditRequest) -> Result<PlanDraftEditResponse, &'static str> {
        let operation = {
            let drafts = self.drafts.lock().map_err(|_| "support.internal_error")?;
            let draft = drafts
                .entries
                .get(&request.draft_ref)
                .ok_or_else(|| missing_draft(&drafts, &request.draft_ref))?;
            if draft.edit_seq != request.edit_seq {
                return Err("plan.draft_stale");
            }
            draft.operation.clone()
        };
        let _operation = operation.lock().map_err(|_| "support.internal_error")?;
        let mut drafts = self.drafts.lock().map_err(|_| "support.internal_error")?;
        let missing = missing_draft(&drafts, &request.draft_ref);
        let Some(draft) = drafts.entries.get_mut(&request.draft_ref) else {
            return Err(missing);
        };
        if draft.edit_seq != request.edit_seq {
            return Err("plan.draft_stale");
        }
        if let Some(build) = &draft.last_build {
            let _ = self.runtime.cancel(&build.job_ref);
        }
        match request.edit {
            PlanDraftEdit::AddItem { item_ref } => {
                if item_ref.result_version != draft.source_version_id {
                    return Err("plan.source_not_current");
                }
                if draft.selections.len() >= du_change_planner::api::MAX_PLAN_ITEMS {
                    return Err(PLAN_TOO_MANY_ITEMS);
                }
                draft.selections.push(selection(item_ref));
            }
            PlanDraftEdit::RemoveItem { item_ref } => {
                if !draft
                    .selections
                    .iter()
                    .any(|selected| selected.item == item_ref)
                {
                    return Err("plan.draft_stale");
                }
                draft
                    .selections
                    .retain(|selected| selected.item != item_ref);
            }
            PlanDraftEdit::SetAction { item_ref, action } => {
                update_item(draft, &item_ref, |selection| {
                    selection.action = match action {
                        du_core_contracts::job::ChangeAction::Trash => PlanAction::Trash,
                        du_core_contracts::job::ChangeAction::Move => PlanAction::Move,
                        du_core_contracts::job::ChangeAction::Delete => PlanAction::Delete,
                    };
                })?;
            }
            PlanDraftEdit::SetDestination {
                item_ref,
                destination,
            } => update_item(draft, &item_ref, |selection| {
                selection.destination = destination.clone();
            })?,
            PlanDraftEdit::SetRetentionCondition {
                item_ref,
                retention_condition_ref,
            } => update_item(draft, &item_ref, |selection| {
                selection.condition =
                    storage::retention_condition_bytes(retention_condition_ref.as_ref());
            })?,
            PlanDraftEdit::SetOverwrite {
                item_ref,
                overwrite,
            } => {
                update_item(draft, &item_ref, |selection| {
                    selection.overwrite = overwrite
                })?;
            }
        }
        draft.edit_seq = draft.edit_seq.checked_add(1).ok_or("plan.draft_stale")?;
        let edit_seq = draft.edit_seq;
        let count = draft.selections.len();
        draft.last_build = None;
        draft.last_fixed = None;
        touch(&mut drafts, &request.draft_ref);
        Ok(PlanDraftEditResponse {
            draft_ref: request.draft_ref,
            edit_seq,
            summary: summary(count),
        })
    }

    fn cancel(
        &self,
        request: PlanDraftCancelRequest,
    ) -> Result<PlanDraftCancelResponse, &'static str> {
        let operation = {
            let drafts = self.drafts.lock().map_err(|_| "support.internal_error")?;
            let draft = drafts
                .entries
                .get(&request.draft_ref)
                .ok_or_else(|| missing_draft(&drafts, &request.draft_ref))?;
            draft.operation.clone()
        };
        let _operation = operation.lock().map_err(|_| "support.internal_error")?;
        let draft = {
            let mut drafts = self.drafts.lock().map_err(|_| "support.internal_error")?;
            let missing = missing_draft(&drafts, &request.draft_ref);
            let Some(draft) = drafts.entries.remove(&request.draft_ref) else {
                return Err(missing);
            };
            drafts.order.retain(|id| id != &request.draft_ref);
            draft
        };
        let mut build_cancelled = false;
        if let Some(build) = draft.last_build {
            build_cancelled = matches!(
                self.runtime.cancel(&build.job_ref),
                Ok(du_job_runtime::CancelResult::Accepted(_))
            );
        }
        Ok(PlanDraftCancelResponse {
            discarded: true,
            build_cancelled,
        })
    }

    fn retire(&self) {
        let jobs = self
            .drafts
            .lock()
            .map(|mut drafts| {
                let jobs = drafts
                    .entries
                    .values()
                    .filter_map(|draft| {
                        draft.last_build.as_ref().map(|build| build.job_ref.clone())
                    })
                    .collect::<Vec<_>>();
                *drafts = Drafts::default();
                jobs
            })
            .unwrap_or_default();
        for job in jobs {
            let _ = self.runtime.cancel(&job);
        }
    }
}

fn selection(item: du_core_contracts::ItemRef) -> PlanSelection {
    PlanSelection {
        item,
        action: PlanAction::Trash,
        condition: Vec::new(),
        selection_evidence: vec!["user_selection".into()],
        destination: None,
        overwrite: false,
        displaced_size: None,
    }
}

fn update_item(
    draft: &mut Draft,
    item_ref: &du_core_contracts::ItemRef,
    mut update: impl FnMut(&mut PlanSelection),
) -> Result<(), &'static str> {
    let mut found = false;
    for item in draft
        .selections
        .iter_mut()
        .filter(|item| &item.item == item_ref)
    {
        update(item);
        found = true;
    }
    found.then_some(()).ok_or("plan.draft_stale")
}

fn summary(count: usize) -> PlanDraftSummary {
    PlanDraftSummary {
        item_count: count.min(u32::MAX as usize) as u32,
        by_source: vec![PlanSourceCount {
            source: PlanSourceKind::Manual,
            count: count.min(u32::MAX as usize) as u32,
        }],
        conversion_held_count: 0,
    }
}

fn touch(drafts: &mut Drafts, id: &Id) {
    drafts.order.retain(|existing| existing != id);
    drafts.order.push_back(id.clone());
}

fn remember_evicted(drafts: &mut Drafts, id: Id) {
    drafts.evicted.retain(|existing| existing != &id);
    drafts.evicted.push_back(id);
    while drafts.evicted.len() > MAX_DRAFTS {
        drafts.evicted.pop_front();
    }
}

fn missing_draft(drafts: &Drafts, id: &Id) -> &'static str {
    if drafts.evicted.contains(id) {
        SESSION_EVICTED
    } else {
        "plan.draft_stale"
    }
}

fn engine_error(reason: &'static str) -> EngineError {
    EngineError::new(reason, Vec::new())
}

struct PlanIds(Arc<dyn JobIdentity>);
impl PlanIdPort for PlanIds {
    fn next_id(&self) -> Result<Id, du_core_contracts::ports::PortError> {
        Ok(self.0.next_id())
    }
}

struct PlanTime(Arc<dyn JobClock>);
impl ClockPort for PlanTime {
    fn now(&self) -> Option<Timestamp> {
        Timestamp::from_unix_millis(self.0.now_ms())
    }
}
