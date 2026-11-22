use std::sync::Arc;

use du_core_contracts::plan_ipc::{
    PLAN_BUILD, PLAN_DRAFT_CANCEL, PLAN_DRAFT_EDIT, PLAN_DRAFT_OPEN, PLAN_GET, PLAN_HISTORY,
    PLAN_ITEM_REASON, PlanBuildRequest, PlanBuildResponse, PlanDraftCancelRequest,
    PlanDraftCancelResponse, PlanDraftEditRequest, PlanDraftEditResponse, PlanDraftOpenRequest,
    PlanDraftOpenResponse, PlanGetRequest, PlanGetResponse, PlanHistoryRequest,
    PlanHistoryResponse, PlanItemReasonRequest, PlanItemReasonResponse,
};

use super::{
    DomainPlatformFactsPort, DomainTargetCheck, PlanService, TargetCheckPort, engine_error, job,
    storage,
};
use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

pub(super) fn register(builder: &mut EngineBuilder, service: Arc<PlanService>) {
    builder.register(DraftOpen(service.clone()));
    builder.register(DraftEditUseCase(service.clone()));
    builder.register(DraftCancel(service.clone()));
    builder.register(Build(service.clone()));
    builder.register(Get(service.clone()));
    builder.register(ItemReason(service.clone()));
    builder.register(History(service));
}
pub(super) struct UnknownTargetCheck;
impl TargetCheckPort for UnknownTargetCheck {
    fn check(
        &self,
        _: &du_change_planner::ports::AnalysisItemFacts,
    ) -> Result<DomainTargetCheck, du_core_contracts::ports::PortError> {
        Ok(DomainTargetCheck::Unavailable)
    }
}

pub(super) struct UnknownPlatformFacts;
impl DomainPlatformFactsPort for UnknownPlatformFacts {
    fn read(
        &self,
        _: &du_change_planner::ports::AnalysisItemFacts,
        _: &du_change_planner::ports::RulesetSnapshot,
        _: du_change_planner::ports::PlanAction,
    ) -> Result<
        du_change_planner::ports::PlatformProtectionFacts,
        du_core_contracts::ports::PortError,
    > {
        use du_change_planner::ports::{Fact, PlatformProtectionFacts};
        use du_core_contracts::protection_facts::fact_reason::UNSUPPORTED;
        Ok(PlatformProtectionFacts {
            identity: Fact::Unknown(UNSUPPORTED),
            link_boundary: Fact::Unknown(UNSUPPORTED),
            volume_role: Fact::Unknown(UNSUPPORTED),
            known_location: Fact::Unknown(UNSUPPORTED),
            classification_id: None,
            os_component: Fact::Unknown(UNSUPPORTED),
            age_guard: Fact::Unknown(UNSUPPORTED),
            permission_guard: Fact::Unknown(UNSUPPORTED),
            disk_backed_guard: Fact::Unknown(UNSUPPORTED),
            user_rule: Fact::Unknown(UNSUPPORTED),
            descendants: Fact::Unknown(UNSUPPORTED),
            trash_capability: None,
        })
    }
}

struct DraftOpen(Arc<PlanService>);
impl UseCase for DraftOpen {
    const NAME: &'static str = PLAN_DRAFT_OPEN;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = PlanDraftOpenRequest;
    type Response = PlanDraftOpenResponse;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0.open(request).map_err(engine_error)
    }
}

struct DraftEditUseCase(Arc<PlanService>);
impl UseCase for DraftEditUseCase {
    const NAME: &'static str = PLAN_DRAFT_EDIT;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = PlanDraftEditRequest;
    type Response = PlanDraftEditResponse;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0.edit(request).map_err(engine_error)
    }
}

struct DraftCancel(Arc<PlanService>);
impl UseCase for DraftCancel {
    const NAME: &'static str = PLAN_DRAFT_CANCEL;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = PlanDraftCancelRequest;
    type Response = PlanDraftCancelResponse;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0.cancel(request).map_err(engine_error)
    }
}

struct Build(Arc<PlanService>);
impl UseCase for Build {
    const NAME: &'static str = PLAN_BUILD;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = PlanBuildRequest;
    type Response = PlanBuildResponse;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        job::submit(&self.0, request, caller).map_err(engine_error)
    }
}

struct Get(Arc<PlanService>);
impl UseCase for Get {
    const NAME: &'static str = PLAN_GET;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = PlanGetRequest;
    type Response = PlanGetResponse;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        storage::read_for_get(&self.0.versions, request).map_err(engine_error)
    }
}

struct ItemReason(Arc<PlanService>);
impl UseCase for ItemReason {
    const NAME: &'static str = PLAN_ITEM_REASON;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = PlanItemReasonRequest;
    type Response = PlanItemReasonResponse;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        storage::read_reason(&self.0.versions, &request.plan_version_id, request.item_no)
            .map_err(engine_error)
    }
}

struct History(Arc<PlanService>);
impl UseCase for History {
    const NAME: &'static str = PLAN_HISTORY;
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = PlanHistoryRequest;
    type Response = PlanHistoryResponse;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        storage::read_history(&self.0.versions, request).map_err(engine_error)
    }
}
