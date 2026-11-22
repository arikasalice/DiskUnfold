//! Pure plan construction and execution-time re-judgement entry points.
mod error;
mod evaluators;
pub use error::PlanError;
pub use evaluators::{EvaluatorRegistry, EvaluatorRegistryError};

use crate::{
    domain::{
        self, evaluate_protection, plan_reason, plan_state, platform_unavailable,
        restore_restriction,
    },
    ports::{
        AnalysisFactsPort, AnalysisItemFacts, ClockPort, ConfirmationChoice, ConfirmationToken,
        PlanAction, PlanIdPort, PlanItem, PlanItemState, PlanRejudgement, PlanSelection,
        PlanSourceKind, PlanVersion, PlanVersionPort, PlatformFactsPort, ProtectionRulesetPort,
        ProtectionSummary, ProtectionVerdict, RejudgementState, RequestChannel, RulesetSnapshot,
        RulesetVersion, TargetCheck, TargetCheckPort,
    },
    readiness,
};
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, ports::PortError};
use std::collections::BTreeSet;

/// Maximum number of selected target rows accepted by F10 plan construction.
pub const MAX_PLAN_ITEMS: usize = 100_000;
/// Canonical hash format tag for immutable plan versions.
pub const PLAN_HASH_FORMAT: &str = "du.plan.hash.v1";

/// Read-only dependencies required by the plan domain.
pub struct PlannerPorts<'a> {
    /// Pinned analysis result facts.
    pub analysis: &'a dyn AnalysisFactsPort,
    /// Current target metadata comparison.
    pub target_check: &'a dyn TargetCheckPort,
    /// Current platform and protection facts.
    pub platform: &'a dyn PlatformFactsPort,
    /// Immutable protection ruleset snapshots.
    pub rulesets: &'a dyn ProtectionRulesetPort,
    /// Stored immutable plan versions.
    pub versions: &'a dyn PlanVersionPort,
    /// Deterministic UTC time.
    pub clock: &'a dyn ClockPort,
    /// UUID v7 allocation.
    pub ids: &'a dyn PlanIdPort,
}

/// Input to the pure version-fixing steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanRequest {
    /// Immutable result version containing every selected item.
    pub source_version_id: du_core_contracts::Id,
    /// Trusted source kind attached by the request adapter.
    pub source_kind: PlanSourceKind,
    /// Request channel used for source admission and output metadata.
    pub request_channel: RequestChannel,
    /// Selected items; no additional targets are inferred.
    pub selections: Vec<PlanSelection>,
    /// User confirmations bound into this plan.
    pub confirmations: Vec<ConfirmationChoice>,
    /// Parent version for an edited plan.
    pub parent_version_id: Option<du_core_contracts::Id>,
}

/// Plan-building and re-judgement service assembled from narrow ports.
pub struct PlanBuilder<'a> {
    ports: PlannerPorts<'a>,
    evaluators: EvaluatorRegistry,
}

impl<'a> PlanBuilder<'a> {
    /// Assembles a plan service after validating the complete evaluator registry.
    pub fn new(
        ports: PlannerPorts<'a>,
        evaluators: EvaluatorRegistry,
    ) -> Result<Self, EvaluatorRegistryError> {
        evaluators.validate()?;
        Ok(Self { ports, evaluators })
    }

    /// Assembles a plan service with the twelve built-in evaluators.
    pub fn standard(ports: PlannerPorts<'a>) -> Self {
        Self {
            ports,
            evaluators: EvaluatorRegistry::standard(),
        }
    }

    /// Recomputes the canonical hash of a plan version without reading ports.
    pub fn hash_version(version: &PlanVersion) -> [u8; 32] {
        domain::plan_hash(version)
    }

    /// Builds one immutable plan value without writing storage or changing files.
    pub fn build(&self, request: PlanRequest) -> Result<PlanVersion, PlanError> {
        validate_request(&request)?;
        let data_source = self
            .ports
            .analysis
            .data_source(&request.source_version_id)
            .map_err(|error| port_failure("analysis.data_source", error))?;
        if readiness::readiness_first_row(Some(data_source))
            == Some(readiness::PlanReadiness::SourceNotCurrent)
        {
            return Err(PlanError::SourceNotCurrent);
        }
        let now = self.ports.clock.now().ok_or(PlanError::ClockUnavailable)?;
        let parent = self.read_parent(request.parent_version_id.as_ref())?;
        let version_id = self.ports.ids.next_id().map_err(PlanError::IdUnavailable)?;
        let lineage_id = parent
            .as_ref()
            .map(|parent| parent.lineage_id.clone())
            .unwrap_or_else(|| version_id.clone());

        let candidates = request
            .selections
            .iter()
            .map(|selection| {
                let facts = self
                    .ports
                    .analysis
                    .read_item(&selection.item)
                    .map_err(|error| port_failure("analysis.item", error))?;
                Ok((selection.clone(), facts))
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        let mut items = domain::normalize(candidates).map_err(|_| PlanError::InvalidSourcePath)?;
        self.check_targets(&mut items);

        let ruleset = self.ports.rulesets.read_current().ok();
        self.evaluate_items(
            &mut items,
            ruleset.as_ref(),
            request
                .confirmations
                .contains(&ConfirmationChoice::ReleaseDefaultProtection),
            request
                .confirmations
                .contains(&ConfirmationChoice::Overwrite),
            Some(&now),
        );
        let confirmations = confirmation_tokens(&items)?;
        let expected_effect = domain::expected_effect(&items);
        let readiness = readiness::evaluate(data_source, &items);
        let mut plan = PlanVersion {
            version_id,
            lineage_id,
            parent_version_id: request.parent_version_id,
            source_version_id: request.source_version_id,
            source_kind: request.source_kind,
            data_source,
            request_channel: request.request_channel,
            ruleset: ruleset.as_ref().map(RulesetVersion::from),
            items,
            expected_effect,
            confirmations,
            readiness,
            hash_format: PLAN_HASH_FORMAT.to_owned(),
            hash: [0; 32],
            created_at: now,
        };
        plan.hash = domain::plan_hash(&plan);
        Ok(plan)
    }

    /// Recomputes a stored version's hash and checks current protection facts.
    pub fn rejudge(
        &self,
        version_id: &du_core_contracts::Id,
    ) -> Result<PlanRejudgement, PlanError> {
        let plan = self
            .ports
            .versions
            .read(version_id)
            .map_err(|_| PlanError::ParentUnavailable)?;
        let recomputed_hash = domain::plan_hash(&plan);
        if recomputed_hash != plan.hash {
            return Err(PlanError::StoredHashMismatch);
        }
        let ruleset = self.ports.rulesets.read_current().ok();
        let now = self.ports.clock.now();
        let mut items = Vec::with_capacity(plan.items.len());
        for item in &plan.items {
            let state = if item.state != PlanItemState::Included {
                RejudgementState::Denied
            } else if let Some(ruleset) = &ruleset {
                self.rejudge_item(item, ruleset, now.as_ref())
            } else {
                RejudgementState::Undetermined
            };
            items.push((item.item_no, state));
        }
        Ok(PlanRejudgement {
            version_id: plan.version_id,
            recomputed_hash,
            ruleset_version: ruleset.as_ref().map(|snapshot| snapshot.version),
            items,
        })
    }

    fn read_parent(
        &self,
        parent_id: Option<&du_core_contracts::Id>,
    ) -> Result<Option<PlanVersion>, PlanError> {
        let Some(parent_id) = parent_id else {
            return Ok(None);
        };
        let parent = self
            .ports
            .versions
            .read(parent_id)
            .map_err(|_| PlanError::ParentUnavailable)?;
        if domain::plan_hash(&parent) != parent.hash {
            return Err(PlanError::ParentUnavailable);
        }
        Ok(Some(parent))
    }

    fn check_targets(&self, items: &mut [PlanItem]) {
        for item in items
            .iter_mut()
            .filter(|item| item.state != PlanItemState::Rejected)
        {
            let facts = analysis_facts(item);
            match self.ports.target_check.check(&facts) {
                Ok(TargetCheck::Matches) => {}
                Ok(TargetCheck::Changed | TargetCheck::Unavailable) | Err(_) => {
                    hold(item, "plan.item.needs_confirmation");
                }
            }
        }
    }

    fn evaluate_items(
        &self,
        items: &mut [PlanItem],
        ruleset: Option<&RulesetSnapshot>,
        release_confirmed: bool,
        overwrite_confirmed: bool,
        now: Option<&du_core_contracts::Timestamp>,
    ) {
        let evaluators = self.evaluators.ordered();
        for item in items
            .iter_mut()
            .filter(|item| item.state != PlanItemState::Rejected)
        {
            if item.action == PlanAction::Move && item.destination.is_none() {
                hold(item, "plan.item.dest_unknown");
            }
            if item.overwrite {
                if !overwrite_confirmed {
                    hold(item, "plan.item.needs_confirmation");
                } else if exact_logical_size(item.displaced_size.as_ref()).is_none() {
                    hold(item, "plan.item.dest_overwrite_unknown");
                }
            }
            let Some(ruleset) = ruleset else {
                let mut summary = platform_unavailable("protect.ruleset_unreadable");
                summary
                    .evidence
                    .push("protect.ruleset_unreadable".to_owned());
                item.restore = restore_restriction(item.action, None);
                apply_protection(item, summary);
                continue;
            };
            let facts = analysis_facts(item);
            let platform = self.ports.platform.read(&facts, ruleset, item.action);
            let summary = match platform {
                Ok(platform) => {
                    item.restore = restore_restriction(item.action, platform.trash_capability);
                    if item.action == PlanAction::Trash
                        && !matches!(
                            platform.trash_capability,
                            Some(du_core_contracts::volume::CapabilityState::Supported)
                                | Some(du_core_contracts::volume::CapabilityState::Partial)
                        )
                    {
                        hold(item, "plan.item.action_unsupported");
                    }
                    evaluate_protection(
                        &evaluators,
                        &facts,
                        &platform,
                        ruleset,
                        item.action,
                        release_confirmed,
                        now,
                    )
                }
                Err(error) => {
                    item.restore = restore_restriction(item.action, None);
                    let mut summary = platform_unavailable("protect.platform_unavailable");
                    summary.evidence.push(error.reason_key);
                    summary
                }
            };
            apply_protection(item, summary);
        }
    }

    fn rejudge_item(
        &self,
        item: &PlanItem,
        ruleset: &RulesetSnapshot,
        now: Option<&du_core_contracts::Timestamp>,
    ) -> RejudgementState {
        let facts = match self.ports.analysis.read_item(&item.item) {
            Ok(facts) => facts,
            Err(_) => return RejudgementState::Undetermined,
        };
        let platform = match self.ports.platform.read(&facts, ruleset, item.action) {
            Ok(platform) => platform,
            Err(_) => return RejudgementState::Undetermined,
        };
        let old_released = item
            .protection
            .as_ref()
            .is_some_and(|summary| summary.released);
        let evaluators = self.evaluators.ordered();
        let current = evaluate_protection(
            &evaluators,
            &facts,
            &platform,
            ruleset,
            item.action,
            old_released,
            now,
        );
        match current.verdict {
            ProtectionVerdict::Deny => RejudgementState::Denied,
            ProtectionVerdict::Undetermined => RejudgementState::Undetermined,
            ProtectionVerdict::GuidanceOnly => RejudgementState::ClassChanged,
            ProtectionVerdict::Allow | ProtectionVerdict::AllowReleased => {
                if item.protection.as_ref() == Some(&current) {
                    RejudgementState::Same
                } else {
                    RejudgementState::ClassChanged
                }
            }
        }
    }
}

impl From<&RulesetSnapshot> for RulesetVersion {
    fn from(value: &RulesetSnapshot) -> Self {
        Self {
            version: value.version,
            hash: value.hash.clone(),
            classification_version: value.classification_version,
            known_location_version: value.known_location_version,
        }
    }
}

fn validate_request(request: &PlanRequest) -> Result<(), PlanError> {
    if request.selections.is_empty() {
        return Err(PlanError::Empty);
    }
    if request.selections.len() > MAX_PLAN_ITEMS {
        return Err(PlanError::TooManyItems);
    }
    if !source_allowed(request.source_kind, request.request_channel) {
        return Err(PlanError::SourceChannelDenied);
    }
    if request
        .selections
        .iter()
        .any(|selection| selection.item.result_version != request.source_version_id)
    {
        return Err(PlanError::SourceNotCurrent);
    }
    Ok(())
}

fn source_allowed(source: PlanSourceKind, channel: RequestChannel) -> bool {
    match source {
        PlanSourceKind::UserSelection => true,
        PlanSourceKind::GoalAlternative
        | PlanSourceKind::KeepSuggestion
        | PlanSourceKind::Scenario => channel == RequestChannel::Gui,
    }
}

fn analysis_facts(item: &PlanItem) -> AnalysisItemFacts {
    AnalysisItemFacts {
        path: item.path.clone(),
        path_components: item.path_components.clone(),
        volume_id: item.volume_id.clone(),
        identity: item.expected_identity.clone(),
        kind: item.expected_kind,
        size: item.expected_size.clone(),
        modified_at: item.expected_modified_at.clone(),
        classification_id: item.classification_id.clone(),
        descendants: item.observed_descendants.unwrap_or(
            du_core_contracts::protection_facts::ProtectionFact::Unknown(
                du_core_contracts::protection_facts::fact_reason::PARTIAL,
            ),
        ),
    }
}

fn hold(item: &mut PlanItem, reason: &'static str) {
    if item.state == PlanItemState::Included {
        item.state = PlanItemState::Held;
        item.reason_key = Some(reason.to_owned());
    }
}

fn apply_protection(item: &mut PlanItem, summary: ProtectionSummary) {
    let next = plan_state(summary.verdict);
    let reason = plan_reason(summary.verdict);
    match next {
        PlanItemState::Rejected => {
            item.state = PlanItemState::Rejected;
            item.reason_key = reason.map(str::to_owned);
        }
        PlanItemState::Held if item.state == PlanItemState::Included => {
            item.state = PlanItemState::Held;
            item.reason_key = reason.map(str::to_owned);
        }
        PlanItemState::Included | PlanItemState::Held => {}
    }
    item.protection = Some(summary);
}

fn confirmation_tokens(items: &[PlanItem]) -> Result<Vec<ConfirmationToken>, PlanError> {
    let mut tokens = Vec::new();
    let released = items
        .iter()
        .filter(|item| item.state == PlanItemState::Included)
        .filter_map(|item| item.protection.as_ref())
        .filter(|summary| summary.released)
        .filter_map(|summary| summary.classification_id.clone())
        .collect::<BTreeSet<_>>();
    if !released.is_empty() {
        tokens.push(ConfirmationToken::ReleaseDefaultProtection {
            classification_ids: released.into_iter().collect(),
        });
    }

    let mut overwritten = 0_u64;
    let mut displaced_bytes = 0_u64;
    for item in items
        .iter()
        .filter(|item| item.state == PlanItemState::Included && item.overwrite)
    {
        overwritten = overwritten
            .checked_add(1)
            .ok_or(PlanError::ConfirmationOverflow)?;
        displaced_bytes = displaced_bytes
            .checked_add(
                exact_logical_size(item.displaced_size.as_ref())
                    .ok_or(PlanError::ConfirmationOverflow)?,
            )
            .ok_or(PlanError::ConfirmationOverflow)?;
    }
    if overwritten > 0 {
        tokens.push(ConfirmationToken::Overwrite {
            item_count: overwritten,
            displaced_bytes,
        });
    }
    Ok(tokens)
}

fn exact_logical_size(size: Option<&du_core_contracts::SizeValue>) -> Option<u64> {
    let size = size?;
    if size.basis != SizeBasis::Logical || size.evidence == SizeEvidence::Unknown {
        return None;
    }
    match &size.amount {
        SizeAmount::Exact { bytes } => Some(bytes.get()),
        SizeAmount::LowerBound { .. } | SizeAmount::Unknown => None,
    }
}

fn port_failure(boundary: &'static str, error: PortError) -> PlanError {
    PlanError::PortFailure { boundary, error }
}
