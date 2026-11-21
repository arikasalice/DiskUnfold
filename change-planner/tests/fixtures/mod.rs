#![allow(dead_code)]

use du_change_planner::{
    api::{PlanBuilder, PlanRequest, PlannerPorts},
    ports::{
        AnalysisFactsPort, AnalysisItemFacts, ClockPort, DescendantResult, EvaluatorError,
        EvaluatorId, Fact, GuardResult, IdentityObservation, KnownLocation, LinkBoundary,
        PlanAction, PlanIdPort, PlanSelection, PlanSourceKind, PlanVersion, PlanVersionPort,
        PlatformFactsPort, PlatformProtectionFacts, ProtectionAction, ProtectionDecision,
        ProtectionEvaluator, ProtectionEvaluatorInput, ProtectionRulesetPort, ProtectionVerdict,
        ProtectionVolumeRole, RequestChannel, RulesetSnapshot, TargetCheck, TargetCheckPort,
        TargetIdentity,
    },
};
use du_core_contracts::{
    ByteSize, Id, ItemRef, OsPath, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence,
    SizeValue, Timestamp,
    data_source::DataSource,
    ports::{PortError, PortErrorKind},
    result::ResultItemKind,
    volume::CapabilityState,
};
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

pub struct FakePorts {
    pub source_version: Id,
    pub source: DataSource,
    pub items: Mutex<HashMap<u32, AnalysisItemFacts>>,
    pub default_item: AnalysisItemFacts,
    pub target_check: TargetCheck,
    pub platform: PlatformProtectionFacts,
    pub platform_error: Option<PortError>,
    pub ruleset: Result<RulesetSnapshot, PortError>,
    pub stored: Mutex<HashMap<Id, PlanVersion>>,
    pub now: Option<Timestamp>,
    next_id: AtomicUsize,
}

impl FakePorts {
    pub fn standard() -> Self {
        let source_version = id(1);
        Self {
            source_version: source_version.clone(),
            source: DataSource::Real,
            items: Mutex::new(HashMap::new()),
            default_item: item_facts(b"/root/file", &[b"root", b"file"], b"file-1", Some(12)),
            target_check: TargetCheck::Matches,
            platform: platform_facts(),
            platform_error: None,
            ruleset: Ok(ruleset()),
            stored: Mutex::new(HashMap::new()),
            now: Timestamp::from_unix_millis(1_700_000_000_000),
            next_id: AtomicUsize::new(2),
        }
    }

    pub fn set_item(&self, item_no: u32, facts: AnalysisItemFacts) {
        if let Ok(mut items) = self.items.lock() {
            items.insert(item_no, facts);
        }
    }

    pub fn save(&self, plan: PlanVersion) {
        if let Ok(mut plans) = self.stored.lock() {
            plans.insert(plan.version_id.clone(), plan);
        }
    }

    pub fn planner(&self) -> PlanBuilder<'_> {
        PlanBuilder::standard(PlannerPorts {
            analysis: self,
            target_check: self,
            platform: self,
            rulesets: self,
            versions: self,
            clock: self,
            ids: self,
        })
    }

    pub fn planner_with(
        &self,
        evaluators: du_change_planner::api::EvaluatorRegistry,
    ) -> Result<PlanBuilder<'_>, du_change_planner::api::EvaluatorRegistryError> {
        PlanBuilder::new(
            PlannerPorts {
                analysis: self,
                target_check: self,
                platform: self,
                rulesets: self,
                versions: self,
                clock: self,
                ids: self,
            },
            evaluators,
        )
    }
}

impl AnalysisFactsPort for FakePorts {
    fn data_source(&self, _version: &Id) -> Result<DataSource, PortError> {
        Ok(self.source)
    }

    fn read_item(&self, item: &ItemRef) -> Result<AnalysisItemFacts, PortError> {
        match self.items.lock() {
            Ok(items) => Ok(items
                .get(&item.item_no)
                .cloned()
                .unwrap_or_else(|| self.default_item.clone())),
            Err(_) => Err(port_error("fake.items_unavailable")),
        }
    }
}

impl TargetCheckPort for FakePorts {
    fn check(&self, _item: &AnalysisItemFacts) -> Result<TargetCheck, PortError> {
        Ok(self.target_check)
    }
}

impl PlatformFactsPort for FakePorts {
    fn read(
        &self,
        _item: &AnalysisItemFacts,
        _ruleset: &RulesetSnapshot,
    ) -> Result<PlatformProtectionFacts, PortError> {
        match &self.platform_error {
            Some(error) => Err(error.clone()),
            None => Ok(self.platform.clone()),
        }
    }
}

impl ProtectionRulesetPort for FakePorts {
    fn read_current(&self) -> Result<RulesetSnapshot, PortError> {
        self.ruleset.clone()
    }
}

impl PlanVersionPort for FakePorts {
    fn read(&self, version_id: &Id) -> Result<PlanVersion, PortError> {
        match self.stored.lock() {
            Ok(plans) => plans
                .get(version_id)
                .cloned()
                .ok_or_else(|| port_error("fake.plan_not_found")),
            Err(_) => Err(port_error("fake.plans_unavailable")),
        }
    }
}

impl ClockPort for FakePorts {
    fn now(&self) -> Option<Timestamp> {
        self.now.clone()
    }
}

impl PlanIdPort for FakePorts {
    fn next_id(&self) -> Result<Id, PortError> {
        let next = self.next_id.fetch_add(1, Ordering::SeqCst) as u8;
        Ok(Id::from_v7_bytes([next; 16]))
    }
}

pub fn request(fake: &FakePorts, selections: Vec<PlanSelection>) -> PlanRequest {
    PlanRequest {
        source_version_id: fake.source_version.clone(),
        source_kind: PlanSourceKind::UserSelection,
        request_channel: RequestChannel::Gui,
        selections,
        confirmations: Vec::new(),
        parent_version_id: None,
    }
}

pub fn selection(fake: &FakePorts, item_no: u32) -> PlanSelection {
    PlanSelection {
        item: ItemRef {
            result_version: fake.source_version.clone(),
            item_no,
        },
        action: PlanAction::Trash,
        condition: b"same-condition".to_vec(),
        selection_evidence: vec![format!("selection.{item_no}")],
        destination: None,
        overwrite: false,
        displaced_size: None,
    }
}

pub fn id(byte: u8) -> Id {
    Id::from_v7_bytes([byte; 16])
}

pub fn item_facts(
    path: &[u8],
    components: &[&[u8]],
    file_id: &[u8],
    size: Option<u64>,
) -> AnalysisItemFacts {
    AnalysisItemFacts {
        path: OsPath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(path),
            display: String::from_utf8_lossy(path).into_owned(),
            lossy: false,
        },
        path_components: components.iter().map(|part| part.to_vec()).collect(),
        volume_id: b"volume-1".to_vec(),
        identity: Some(TargetIdentity {
            volume_id: b"volume-1".to_vec(),
            file_id: file_id.to_vec(),
        }),
        kind: ResultItemKind::File,
        size: size.map(size_value),
        modified_at: Timestamp::from_unix_millis(1_600_000_000_000),
        classification_id: Some("user-file".to_owned()),
    }
}

pub fn size_value(bytes: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(bytes),
        },
    }
}

pub fn platform_facts() -> PlatformProtectionFacts {
    PlatformProtectionFacts {
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
    }
}

pub fn ruleset() -> RulesetSnapshot {
    RulesetSnapshot {
        version: 4,
        hash: b"ruleset-hash".to_vec(),
        classification_version: 2,
        known_location_version: 3,
        class_actions: [("user-file".to_owned(), ProtectionAction::GuidanceOnly)]
            .into_iter()
            .collect(),
    }
}

pub fn port_error(reason_key: &str) -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: reason_key.to_owned(),
        os_code: None,
    }
}

pub struct ProbeEvaluator {
    pub id: EvaluatorId,
    pub decision: ProtectionVerdict,
    pub failure: Option<&'static str>,
}

impl ProtectionEvaluator for ProbeEvaluator {
    fn id(&self) -> EvaluatorId {
        self.id
    }

    fn evaluate(
        &self,
        _input: &ProtectionEvaluatorInput<'_>,
    ) -> Result<ProtectionDecision, EvaluatorError> {
        if let Some(reason_key) = self.failure {
            return Err(EvaluatorError {
                reason_key: reason_key.to_owned(),
            });
        }
        Ok(ProtectionDecision {
            verdict: self.decision,
            reason_key: None,
        })
    }
}

pub fn probe_registry(failing: Option<EvaluatorId>) -> du_change_planner::api::EvaluatorRegistry {
    probe_registry_with(failing, &[])
}

pub fn probe_registry_with(
    failing: Option<EvaluatorId>,
    decisions: &[(EvaluatorId, ProtectionVerdict)],
) -> du_change_planner::api::EvaluatorRegistry {
    let mut registry = du_change_planner::api::EvaluatorRegistry::empty();
    for id in EvaluatorId::ORDERED {
        let evaluator = ProbeEvaluator {
            id,
            decision: decisions
                .iter()
                .find_map(|(candidate, verdict)| (*candidate == id).then_some(*verdict))
                .unwrap_or(ProtectionVerdict::Allow),
            failure: (Some(id) == failing).then_some("protect.injected_failure"),
        };
        assert!(registry.register(Box::new(evaluator)).is_ok());
    }
    registry
}
