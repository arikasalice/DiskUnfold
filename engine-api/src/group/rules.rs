//! `group.rules.get` and `group.rules.save`: one ruleset per profile.
use std::sync::Arc;

use du_analysis_index::group::ruleset::{
    RuleDraft, RuleLimits, SavePlan, plan_save, review_stored,
};
use du_core_contracts::{
    group_ipc::{
        GroupRuleIssue, GroupRulesGetRequest, GroupRulesSaveRequest, GroupRulesSaveResponse,
        GroupRuleset,
    },
    group_rules::GroupRulesSaveOutcome,
};

use super::{Services, codec, internal};
use crate::{
    api::{CallerCtx, Channel, UseCase},
    browse::folder::lower_condition,
    error::EngineError,
};

const RULE_INVALID: &str = "group.rule_invalid";

pub(super) struct Get(pub(super) Arc<Services>);

impl UseCase for Get {
    const NAME: &'static str = "group.rules.get";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = GroupRulesGetRequest;
    type Response = GroupRuleset;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        let loaded = codec::load(self.0.rules.as_ref())?;
        let reviews = review_stored(&self.0.registry, &loaded.ruleset.rules);
        Ok(codec::response(&loaded.ruleset, &loaded.wire, &reviews))
    }
}

pub(super) struct Save(pub(super) Arc<Services>);

impl UseCase for Save {
    const NAME: &'static str = "group.rules.save";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = GroupRulesSaveRequest;
    type Response = GroupRulesSaveResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let services = &self.0;
        let loaded = codec::load(services.rules.as_ref())?;
        let mut issues = Vec::new();
        let mut drafts = Vec::with_capacity(request.rules.len());
        let mut wire = Vec::with_capacity(request.rules.len());
        for (index, draft) in request.rules.into_iter().enumerate() {
            match lower_condition(draft.condition.clone()) {
                Ok(condition) => drafts.push(RuleDraft {
                    group_id: draft.group_id,
                    display_name: draft.display_name,
                    condition,
                }),
                Err(_) => issues.push(issue(index, "query.syntax_invalid")),
            }
            wire.push(draft.condition);
        }
        if !issues.is_empty() {
            return Ok(rejected(issues));
        }
        let ids = services.assembly.ids.clone();
        let plan = plan_save(
            &loaded.ruleset,
            drafts,
            &services.registry,
            RuleLimits::default(),
            services.assembly.clock.now_ms(),
            &mut || ids.next_id(),
        );
        match plan {
            SavePlan::Unchanged => {
                let reviews = review_stored(&services.registry, &loaded.ruleset.rules);
                Ok(GroupRulesSaveResponse::Saved {
                    ruleset: codec::response(&loaded.ruleset, &loaded.wire, &reviews),
                })
            }
            SavePlan::Rejected(found) => Ok(rejected(
                found
                    .into_iter()
                    .map(|found| issue(found.index, found.reason_key))
                    .collect(),
            )),
            SavePlan::Write(next) => {
                if request.expected_revision != loaded.ruleset.revision {
                    return Err(EngineError::reason("group.rules_modified"));
                }
                let ruleset_id = loaded.ruleset_id.unwrap_or_else(|| ids.next_id());
                let stored = codec::encode(ruleset_id, &next, &wire)?;
                match services.rules.replace(request.expected_revision, &stored) {
                    GroupRulesSaveOutcome::Saved => Ok(GroupRulesSaveResponse::Saved {
                        ruleset: codec::response(&next, &wire, &[]),
                    }),
                    GroupRulesSaveOutcome::Conflict { .. } => {
                        Err(EngineError::reason("group.rules_modified"))
                    }
                    GroupRulesSaveOutcome::Busy => Err(EngineError::reason("db.busy")),
                    GroupRulesSaveOutcome::Failed => Err(internal()),
                }
            }
        }
    }
}

fn issue(index: usize, reason_key: &str) -> GroupRuleIssue {
    GroupRuleIssue {
        index: u32::try_from(index).unwrap_or(u32::MAX),
        reason_key: reason_key.into(),
    }
}

fn rejected(issues: Vec<GroupRuleIssue>) -> GroupRulesSaveResponse {
    GroupRulesSaveResponse::Rejected {
        reason_key: RULE_INVALID.into(),
        issues,
    }
}
