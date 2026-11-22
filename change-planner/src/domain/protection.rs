use crate::ports::{
    AnalysisItemFacts, ClassReleaseState, DescendantResult, EvaluatorError, EvaluatorId, Fact,
    GuardResult, IdentityObservation, KnownLocation, LinkBoundary, PlanAction,
    PlatformProtectionFacts, ProtectionAction, ProtectionDecision, ProtectionEvaluator,
    ProtectionEvaluatorInput, ProtectionSummary, ProtectionVerdict, ProtectionVolumeRole,
    RulesetSnapshot,
};
/// Runs protection evaluation using one fixed ruleset and current facts.
pub(crate) fn evaluate_protection(
    registered: &[&dyn ProtectionEvaluator],
    analysis: &AnalysisItemFacts,
    platform: &PlatformProtectionFacts,
    ruleset: &RulesetSnapshot,
    action: PlanAction,
    release_confirmed: bool,
    now: Option<&du_core_contracts::Timestamp>,
) -> ProtectionSummary {
    let input = ProtectionEvaluatorInput {
        analysis,
        platform,
        ruleset,
        action,
        release_confirmed,
        now,
    };
    let mut verdict = ProtectionVerdict::Allow;
    let mut evidence = Vec::new();
    let mut evaluated = Vec::new();
    let mut skip_class_rows = false;

    for evaluator in registered {
        let id = evaluator.id();
        if skip_class_rows
            && matches!(
                id,
                EvaluatorId::OsComponent | EvaluatorId::ClassAction | EvaluatorId::ReleaseState
            )
        {
            continue;
        }
        evaluated.push((id, 1));
        match evaluator.evaluate(&input) {
            Ok(decision) => {
                if let Some(reason_key) = decision.reason_key {
                    evidence.push(reason_key);
                }
                skip_class_rows |= decision.skip_class_rows;
                combine(&mut verdict, decision.verdict);
                if verdict == ProtectionVerdict::Deny {
                    break;
                }
            }
            Err(error) => {
                evidence.push(error.reason_key);
                combine(&mut verdict, ProtectionVerdict::Undetermined);
            }
        }
    }
    let released = verdict == ProtectionVerdict::AllowReleased;
    ProtectionSummary {
        verdict,
        classification_id: platform.classification_id.clone(),
        released,
        evaluators: evaluated,
        evidence,
    }
}

fn combine(current: &mut ProtectionVerdict, next: ProtectionVerdict) {
    let rank = |value| match value {
        ProtectionVerdict::Deny => 5,
        ProtectionVerdict::Undetermined => 4,
        ProtectionVerdict::GuidanceOnly => 3,
        ProtectionVerdict::AllowReleased => 2,
        ProtectionVerdict::Allow => 1,
    };
    if rank(next) > rank(*current) {
        *current = next;
    }
}

struct BuiltinEvaluator(EvaluatorId);

impl ProtectionEvaluator for BuiltinEvaluator {
    fn id(&self) -> EvaluatorId {
        self.0
    }

    fn evaluate(
        &self,
        input: &ProtectionEvaluatorInput<'_>,
    ) -> Result<ProtectionDecision, EvaluatorError> {
        let _requested_action = input.action;
        Ok(match self.0 {
            EvaluatorId::Identity => identity(input.analysis, input.platform),
            EvaluatorId::LinkBoundary => link_boundary(input.platform),
            EvaluatorId::VolumeRole => volume_role(input.platform),
            EvaluatorId::KnownLocation => known_location(input.platform),
            EvaluatorId::OsComponent => os_component(input.platform),
            EvaluatorId::ClassAction => class_action(input.platform, input.ruleset),
            EvaluatorId::ReleaseState => {
                release_state(input.ruleset, input.release_confirmed, input.platform)
            }
            EvaluatorId::GuardAge => guard(
                input.platform.age_guard,
                ProtectionVerdict::GuidanceOnly,
                "protect.guidance_only",
            ),
            EvaluatorId::GuardPermission => permission_guard(input.platform.permission_guard),
            EvaluatorId::GuardDiskBacked => guard(
                input.platform.disk_backed_guard,
                ProtectionVerdict::Deny,
                "protect.no_change",
            ),
            EvaluatorId::UserRule => guard(
                input.platform.user_rule,
                ProtectionVerdict::Deny,
                "protect.user_rule",
            ),
            EvaluatorId::Descendant => descendant(input.platform),
        })
    }
}

pub(crate) fn standard_evaluators() -> impl Iterator<Item = Box<dyn ProtectionEvaluator>> {
    EvaluatorId::ORDERED
        .into_iter()
        .map(|id| Box::new(BuiltinEvaluator(id)) as Box<dyn ProtectionEvaluator>)
}

fn decision(verdict: ProtectionVerdict, reason_key: Option<&str>) -> ProtectionDecision {
    ProtectionDecision {
        verdict,
        skip_class_rows: false,
        reason_key: reason_key.map(str::to_owned),
    }
}

fn identity(
    analysis: &AnalysisItemFacts,
    platform: &PlatformProtectionFacts,
) -> ProtectionDecision {
    if analysis.identity.is_none() {
        return decision(
            ProtectionVerdict::Undetermined,
            Some("protect.identity_unavailable"),
        );
    }
    match platform.identity {
        Fact::Known(IdentityObservation::Matches) => decision(ProtectionVerdict::Allow, None),
        Fact::Known(IdentityObservation::Mismatch) => decision(
            ProtectionVerdict::Deny,
            Some("protect.identity_unavailable"),
        ),
        Fact::Unknown(reason) => decision(ProtectionVerdict::Undetermined, Some(reason)),
    }
}

fn link_boundary(platform: &PlatformProtectionFacts) -> ProtectionDecision {
    match platform.link_boundary {
        Fact::Known(LinkBoundary::WithinVolume) => decision(ProtectionVerdict::Allow, None),
        Fact::Known(LinkBoundary::OutsideVolume) => {
            decision(ProtectionVerdict::Deny, Some("protect.link_boundary"))
        }
        Fact::Unknown(reason) => decision(ProtectionVerdict::Undetermined, Some(reason)),
    }
}

fn volume_role(platform: &PlatformProtectionFacts) -> ProtectionDecision {
    match platform.volume_role {
        Fact::Known(ProtectionVolumeRole::Ordinary) => decision(ProtectionVerdict::Allow, None),
        Fact::Known(ProtectionVolumeRole::OsOnly | ProtectionVolumeRole::SystemPartition) => {
            decision(ProtectionVerdict::Deny, Some("protect.locked"))
        }
        Fact::Known(
            ProtectionVolumeRole::VirtualFilesystem | ProtectionVolumeRole::MemoryTemporary,
        ) => decision(ProtectionVerdict::Deny, Some("protect.no_change")),
        Fact::Unknown(reason) => decision(ProtectionVerdict::Undetermined, Some(reason)),
    }
}

fn known_location(platform: &PlatformProtectionFacts) -> ProtectionDecision {
    match platform.known_location {
        Fact::Known(KnownLocation::Root) => {
            decision(ProtectionVerdict::Deny, Some("protect.no_change"))
        }
        Fact::Known(KnownLocation::Under(_)) => decision(ProtectionVerdict::Allow, None),
        Fact::Known(KnownLocation::NotListed) => ProtectionDecision {
            verdict: ProtectionVerdict::Allow,
            skip_class_rows: true,
            reason_key: None,
        },
        Fact::Unknown(reason) => decision(ProtectionVerdict::Undetermined, Some(reason)),
    }
}

fn os_component(platform: &PlatformProtectionFacts) -> ProtectionDecision {
    match platform.os_component {
        Fact::Known(true) => decision(ProtectionVerdict::Deny, Some("protect.locked")),
        Fact::Known(false) => decision(ProtectionVerdict::Allow, None),
        Fact::Unknown(reason) => decision(ProtectionVerdict::Undetermined, Some(reason)),
    }
}

fn class_action(
    platform: &PlatformProtectionFacts,
    ruleset: &RulesetSnapshot,
) -> ProtectionDecision {
    let Some(classification) = platform.classification_id.as_ref() else {
        return decision(
            ProtectionVerdict::Undetermined,
            Some("protect.class_unverifiable"),
        );
    };
    match ruleset.class_actions.get(classification) {
        Some(ProtectionAction::Locked | ProtectionAction::Deny) => {
            decision(ProtectionVerdict::Deny, Some("protect.locked"))
        }
        Some(ProtectionAction::NoChange) => {
            decision(ProtectionVerdict::Deny, Some("protect.no_change"))
        }
        Some(ProtectionAction::Allow) => decision(ProtectionVerdict::Allow, None),
        Some(ProtectionAction::GuidanceOnly) => decision(
            ProtectionVerdict::GuidanceOnly,
            Some("protect.guidance_only"),
        ),
        None => decision(
            ProtectionVerdict::Undetermined,
            Some("protect.class_unverifiable"),
        ),
    }
}

fn release_state(
    ruleset: &RulesetSnapshot,
    _release_confirmed: bool,
    platform: &PlatformProtectionFacts,
) -> ProtectionDecision {
    let Some(classification) = platform.classification_id.as_ref() else {
        return decision(
            ProtectionVerdict::Undetermined,
            Some("protect.class_unverifiable"),
        );
    };
    match (
        ruleset.class_actions.get(classification),
        ruleset.class_release_state.get(classification),
    ) {
        (Some(ProtectionAction::Allow), Some(ClassReleaseState::Released)) => {
            decision(ProtectionVerdict::AllowReleased, None)
        }
        (Some(ProtectionAction::Allow), Some(ClassReleaseState::NotReleasable)) => {
            decision(ProtectionVerdict::Allow, None)
        }
        (Some(ProtectionAction::Deny), Some(ClassReleaseState::Protected)) => {
            decision(ProtectionVerdict::Deny, Some("protect.locked"))
        }
        (Some(ProtectionAction::GuidanceOnly), Some(ClassReleaseState::NotReleasable)) => decision(
            ProtectionVerdict::GuidanceOnly,
            Some("protect.guidance_only"),
        ),
        (
            Some(ProtectionAction::Locked | ProtectionAction::NoChange),
            Some(ClassReleaseState::NotReleasable),
        ) => decision(ProtectionVerdict::Allow, None),
        _ => decision(
            ProtectionVerdict::Undetermined,
            Some("protect.class_unverifiable"),
        ),
    }
}

fn guard(
    result: Fact<GuardResult>,
    protected: ProtectionVerdict,
    protected_reason: &'static str,
) -> ProtectionDecision {
    match result {
        Fact::Known(GuardResult::Clear) => decision(ProtectionVerdict::Allow, None),
        Fact::Known(GuardResult::Protected) => decision(protected, Some(protected_reason)),
        Fact::Unknown(reason) => decision(ProtectionVerdict::Undetermined, Some(reason)),
    }
}

fn permission_guard(result: Fact<GuardResult>) -> ProtectionDecision {
    match result {
        Fact::Known(GuardResult::Clear) => decision(ProtectionVerdict::Allow, None),
        Fact::Known(GuardResult::Protected) => {
            decision(ProtectionVerdict::Deny, Some("protect.permission_denied"))
        }
        Fact::Unknown(reason) => decision(ProtectionVerdict::Undetermined, Some(reason)),
    }
}

fn descendant(platform: &PlatformProtectionFacts) -> ProtectionDecision {
    match platform.descendants {
        Fact::Known(DescendantResult::Clear) => decision(ProtectionVerdict::Allow, None),
        Fact::Known(DescendantResult::Protected) => decision(
            ProtectionVerdict::Deny,
            Some("protect.protected_descendant"),
        ),
        Fact::Known(DescendantResult::Incomplete) => decision(
            ProtectionVerdict::Undetermined,
            Some("protect.descendant_unverifiable"),
        ),
        Fact::Unknown(reason) => decision(ProtectionVerdict::Undetermined, Some(reason)),
    }
}

/// Maps the strongest protection result to the plan-level reason namespace.
pub(crate) fn plan_reason(verdict: ProtectionVerdict) -> Option<&'static str> {
    match verdict {
        ProtectionVerdict::Deny => Some("plan.item.protected"),
        ProtectionVerdict::Undetermined => Some("plan.item.undetermined"),
        ProtectionVerdict::GuidanceOnly => Some("plan.item.guidance_only"),
        ProtectionVerdict::Allow | ProtectionVerdict::AllowReleased => None,
    }
}

/// Reports the plan disposition implied by a protection verdict.
pub(crate) fn plan_state(verdict: ProtectionVerdict) -> crate::ports::PlanItemState {
    use crate::ports::PlanItemState::{Held, Included, Rejected};
    match verdict {
        ProtectionVerdict::Deny | ProtectionVerdict::Undetermined => Rejected,
        ProtectionVerdict::GuidanceOnly => Held,
        ProtectionVerdict::Allow | ProtectionVerdict::AllowReleased => Included,
    }
}

/// Converts a missing platform observation to an undetermined summary.
pub(crate) fn platform_unavailable(reason_key: &str) -> ProtectionSummary {
    ProtectionSummary {
        verdict: ProtectionVerdict::Undetermined,
        classification_id: None,
        released: false,
        evaluators: Vec::new(),
        evidence: vec![reason_key.to_owned()],
    }
}
