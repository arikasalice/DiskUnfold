//! Converts rules between the storage port, the analysis model and the IPC contract.
use du_analysis_index::group::ruleset::{Rule, RuleReview, Ruleset};
use du_core_contracts::{
    Id, Timestamp,
    browse_open_ipc::QueryCondition,
    browse_page_ipc::QueryStrategy,
    group_ipc::{GROUP_CONDITION_MODEL_VERSION, GroupRule, GroupRuleReview, GroupRuleset},
    group_rules::{GroupRulesReadError, GroupRulesStore, StoredGroupRule, StoredGroupRuleset},
};

use super::{hex, internal};
use crate::{browse::folder::lower_condition, error::EngineError};

/// The stored ruleset with the user's wire conditions in display order.
pub(super) struct Loaded {
    pub(super) ruleset_id: Option<Id>,
    pub(super) ruleset: Ruleset,
    pub(super) wire: Vec<QueryCondition>,
}

/// Reads the stored ruleset; a missing ruleset is the empty version zero.
pub(super) fn load(store: &dyn GroupRulesStore) -> Result<Loaded, EngineError> {
    match store.read() {
        Ok(None) => Ok(Loaded {
            ruleset_id: None,
            ruleset: Ruleset::empty(),
            wire: Vec::new(),
        }),
        Ok(Some(stored)) => decode(stored),
        Err(GroupRulesReadError::Busy) => Err(EngineError::reason("db.busy")),
        Err(GroupRulesReadError::Corrupt | GroupRulesReadError::Unavailable) => Err(internal()),
    }
}

/// Decodes a stored ruleset. A format this build does not define (written by a newer
/// build, or an older format without a conversion) is refused with `db.schema_mismatch`,
/// so it is neither shown as empty nor overwritten by a save.
fn decode(stored: StoredGroupRuleset) -> Result<Loaded, EngineError> {
    if stored.format_version != du_analysis_index::group::ruleset::RULESET_FORMAT_VERSION {
        return Err(EngineError::reason("db.schema_mismatch"));
    }
    let mut rules = Vec::with_capacity(stored.rules.len());
    let mut wire = Vec::with_capacity(stored.rules.len());
    for row in stored.rules {
        let condition: QueryCondition =
            serde_json::from_str(&row.condition_json).map_err(|_| internal())?;
        let predicates: Vec<QueryStrategy> =
            serde_json::from_str(&row.predicates_json).map_err(|_| internal())?;
        let domain = lower_condition(condition.clone()).map_err(|_| internal())?;
        rules.push(Rule {
            group_id: row.group_id,
            display_name: row.display_name,
            condition: domain.normalize(),
            predicates: predicates
                .into_iter()
                .map(|strategy| (strategy.id, strategy.revision))
                .collect(),
        });
        wire.push(condition);
    }
    Ok(Loaded {
        ruleset_id: Some(stored.ruleset_id),
        ruleset: Ruleset {
            version_no: stored.version_no,
            revision: stored.revision,
            fingerprint: stored.fingerprint,
            created_at_ms: Some(stored.created_at_ms),
            updated_at_ms: Some(stored.updated_at_ms),
            rules,
        },
        wire,
    })
}

/// Encodes the next version for the storage port. `wire` follows rule order.
pub(super) fn encode(
    ruleset_id: Id,
    next: &Ruleset,
    wire: &[QueryCondition],
) -> Result<StoredGroupRuleset, EngineError> {
    let updated = next.updated_at_ms.ok_or_else(internal)?;
    let mut rules = Vec::with_capacity(next.rules.len());
    for (index, (rule, condition)) in next.rules.iter().zip(wire).enumerate() {
        rules.push(StoredGroupRule {
            group_id: rule.group_id.clone(),
            display_name: rule.display_name.clone(),
            display_order: u32::try_from(index).map_err(|_| internal())?,
            condition_json: serde_json::to_string(condition).map_err(|_| internal())?,
            condition_model_version: GROUP_CONDITION_MODEL_VERSION,
            predicates_json: serde_json::to_string(&strategies(rule)).map_err(|_| internal())?,
        });
    }
    if rules.len() != next.rules.len() {
        return Err(internal());
    }
    Ok(StoredGroupRuleset {
        ruleset_id,
        format_version: du_analysis_index::group::ruleset::RULESET_FORMAT_VERSION,
        version_no: next.version_no,
        revision: next.revision,
        fingerprint: next.fingerprint,
        created_at_ms: next.created_at_ms.unwrap_or(updated),
        updated_at_ms: updated,
        rules,
    })
}

fn strategies(rule: &Rule) -> Vec<QueryStrategy> {
    rule.predicates
        .iter()
        .map(|(id, revision)| QueryStrategy {
            id: id.clone(),
            revision: *revision,
        })
        .collect()
}

/// Builds the IPC ruleset. Missing reviews are reported as `Ok`.
pub(super) fn response(
    ruleset: &Ruleset,
    wire: &[QueryCondition],
    reviews: &[RuleReview],
) -> GroupRuleset {
    let rules = ruleset
        .rules
        .iter()
        .zip(wire)
        .enumerate()
        .map(|(index, (rule, condition))| GroupRule {
            group_id: rule.group_id.clone(),
            display_name: rule.display_name.clone(),
            display_order: u32::try_from(index).unwrap_or(u32::MAX),
            condition: condition.clone(),
            condition_model_version: GROUP_CONDITION_MODEL_VERSION,
            predicates: strategies(rule),
            review: match reviews.get(index) {
                Some(RuleReview::PredicateUnregistered) => GroupRuleReview::PredicateUnregistered,
                Some(RuleReview::Ok) | None => GroupRuleReview::Ok,
            },
        })
        .collect();
    GroupRuleset {
        ruleset_version: ruleset.version_no,
        revision: ruleset.revision,
        fingerprint: hex(&ruleset.fingerprint),
        updated_at: ruleset.updated_at_ms.and_then(Timestamp::from_unix_millis),
        rules,
    }
}
