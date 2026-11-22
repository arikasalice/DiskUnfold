//! Canonical protection ruleset encoding and storage-row verification (F29).

use std::collections::BTreeMap;

use du_core_contracts::ruleset::{
    ClassReleaseState, RulesetAction, RulesetHashFormat, RulesetOrigin, RulesetReadError,
    RulesetRow, RulesetSnapshot, UserProtectionRule, UserRuleKind, classification_id,
};
use serde::{Deserialize, Serialize};

const RULE_FORMAT_VERSION: i64 = 1;
const CLASS_TABLE_VERSION: &str = "1";
const KNOWN_LOCATION_TABLE_VERSION: &str = "1";
const RELEASABLE: &[&str] = &[
    classification_id::COMPONENT_UNCONFIRMED,
    classification_id::DIRECT_CLEANABLE,
    classification_id::SYSTEM_CACHE_CLEANABLE,
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesetBody {
    class_actions: BTreeMap<String, RulesetAction>,
    class_release_state: BTreeMap<String, ClassReleaseState>,
    user_rules: Vec<UserRuleBody>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserRuleBody {
    allowed_operations: Vec<String>,
    kind: UserRuleKind,
    memo: String,
    path_bytes: String,
    target_file_id: Option<String>,
    volume_id: String,
}

/// Produces the complete built-in v1 row for the GUI startup initializer.
pub fn built_in_v1_row(created_at_ms: i64) -> Result<RulesetRow, RulesetReadError> {
    let body = canonical_ruleset_json_v1(built_in_body())?;
    Ok(RulesetRow {
        version: 1,
        ruleset_hash: hash_ruleset_v1(body.as_bytes()).to_vec(),
        hash_format: RulesetHashFormat::DuProtectRulesetV1.as_str().into(),
        class_table_version: CLASS_TABLE_VERSION.into(),
        known_location_table_version: KNOWN_LOCATION_TABLE_VERSION.into(),
        rule_format_version: RULE_FORMAT_VERSION,
        origin: "product_update".into(),
        change_summary: "{}".into(),
        body,
        created_at_ms,
    })
}

/// Verifies every supported row tag, version, body invariant and canonical hash.
pub fn verify_ruleset_row(row: &RulesetRow) -> Result<RulesetSnapshot, RulesetReadError> {
    if row.version <= 0
        || row.ruleset_hash.len() != 32
        || row.hash_format != RulesetHashFormat::DuProtectRulesetV1.as_str()
        || row.class_table_version != CLASS_TABLE_VERSION
        || row.known_location_table_version != KNOWN_LOCATION_TABLE_VERSION
        || row.rule_format_version != RULE_FORMAT_VERSION
        || !matches!(row.origin.as_str(), "product_update" | "gui" | "import")
        || serde_json::from_str::<serde_json::Value>(&row.change_summary).is_err()
    {
        return Err(RulesetReadError::Corrupt);
    }

    let parsed: RulesetBody =
        serde_json::from_str(&row.body).map_err(|_| RulesetReadError::Corrupt)?;
    validate_body(&parsed)?;
    if row.version == 1 && (row.origin != "product_update" || parsed != built_in_body()) {
        return Err(RulesetReadError::Corrupt);
    }
    let canonical = canonical_ruleset_json_v1(parsed.clone())?;
    if canonical.as_bytes() != row.body.as_bytes()
        || hash_ruleset_v1(canonical.as_bytes()).as_slice() != row.ruleset_hash
    {
        return Err(RulesetReadError::Corrupt);
    }

    let version = u64::try_from(row.version)
        .ok()
        .and_then(du_core_contracts::ruleset::RulesetVersion::new)
        .ok_or(RulesetReadError::Corrupt)?;
    let ruleset_hash = row
        .ruleset_hash
        .as_slice()
        .try_into()
        .map_err(|_| RulesetReadError::Corrupt)?;
    Ok(snapshot(row, parsed, version, ruleset_hash))
}

/// Encodes the v1 JSON body with fixed field order, sorted maps and sorted rule data.
fn canonical_ruleset_json_v1(mut canonical: RulesetBody) -> Result<String, RulesetReadError> {
    for rule in &mut canonical.user_rules {
        rule.allowed_operations
            .sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        rule.allowed_operations.dedup();
    }
    let mut rules = canonical
        .user_rules
        .into_iter()
        .map(|rule| {
            serde_json::to_vec(&rule)
                .map(|bytes| (bytes, rule))
                .map_err(|_| RulesetReadError::Corrupt)
        })
        .collect::<Result<Vec<_>, _>>()?;
    rules.sort_by(|left, right| left.0.cmp(&right.0));
    canonical.user_rules = rules.into_iter().map(|(_, rule)| rule).collect();
    serde_json::to_string(&canonical).map_err(|_| RulesetReadError::Corrupt)
}

/// Hashes only canonical v1 body bytes; the format tag is not part of the input.
pub fn hash_ruleset_v1(canonical_body: &[u8]) -> [u8; 32] {
    *blake3::hash(canonical_body).as_bytes()
}

fn validate_body(body: &RulesetBody) -> Result<(), RulesetReadError> {
    let expected = classification_id::ALL.to_vec();
    if body
        .class_actions
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        != expected
        || body
            .class_release_state
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            != expected
    {
        return Err(RulesetReadError::Corrupt);
    }

    for id in classification_id::ALL {
        let action = body
            .class_actions
            .get(*id)
            .ok_or(RulesetReadError::Corrupt)?;
        let state = body
            .class_release_state
            .get(*id)
            .ok_or(RulesetReadError::Corrupt)?;
        match state {
            ClassReleaseState::Released
                if !RELEASABLE.contains(id) || *action != RulesetAction::Allow =>
            {
                return Err(RulesetReadError::Corrupt);
            }
            ClassReleaseState::Protected
                if !RELEASABLE.contains(id) || *action != RulesetAction::Deny =>
            {
                return Err(RulesetReadError::Corrupt);
            }
            ClassReleaseState::NotReleasable if RELEASABLE.contains(id) => {
                return Err(RulesetReadError::Corrupt);
            }
            _ => {}
        }
    }

    for rule in &body.user_rules {
        if !valid_hex(&rule.volume_id)
            || !valid_hex(&rule.path_bytes)
            || rule
                .target_file_id
                .as_ref()
                .is_some_and(|value| !valid_hex(value))
            || (rule.kind != UserRuleKind::AllowedOperations && !rule.allowed_operations.is_empty())
        {
            return Err(RulesetReadError::Corrupt);
        }
    }
    Ok(())
}

fn valid_hex(value: &str) -> bool {
    !value.is_empty()
        && value.len().is_multiple_of(2)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn snapshot(
    row: &RulesetRow,
    body: RulesetBody,
    version: du_core_contracts::ruleset::RulesetVersion,
    ruleset_hash: [u8; 32],
) -> RulesetSnapshot {
    let rules = body
        .user_rules
        .into_iter()
        .map(|rule| UserProtectionRule {
            allowed_operations: rule.allowed_operations,
            kind: rule.kind,
            memo: rule.memo,
            path_bytes: rule.path_bytes,
            target_file_id: rule.target_file_id,
            volume_id: rule.volume_id,
        })
        .collect::<Vec<_>>();
    RulesetSnapshot {
        version,
        ruleset_hash,
        hash_format: RulesetHashFormat::DuProtectRulesetV1,
        class_table_version: du_core_contracts::ruleset::ClassTableVersion::V1,
        known_location_table_version: du_core_contracts::ruleset::KnownLocationTableVersion::V1,
        rule_format_version: du_core_contracts::ruleset::RuleFormatVersion::V1,
        origin: if row.origin == "product_update" {
            RulesetOrigin::ProductUpdate
        } else {
            RulesetOrigin::UserChange
        },
        user_rules: rules,
        class_actions: body.class_actions,
        class_release_state: body.class_release_state,
    }
}

fn built_in_body() -> RulesetBody {
    use ClassReleaseState::{NotReleasable, Protected};
    use RulesetAction::{Allow, Deny, GuidanceOnly, Locked, NoChange};

    let actions = BTreeMap::from([
        (classification_id::ROOT.into(), NoChange),
        (classification_id::OS_COMPONENT.into(), Locked),
        (classification_id::COMPONENT_UNCONFIRMED.into(), Deny),
        (classification_id::MEMORY_FILE.into(), GuidanceOnly),
        (classification_id::BACKUP_SNAPSHOT.into(), GuidanceOnly),
        (classification_id::UPDATE_RESIDUE.into(), GuidanceOnly),
        (classification_id::LOG_DIAGNOSTICS.into(), GuidanceOnly),
        (classification_id::DIRECT_CLEANABLE.into(), Deny),
        (classification_id::SYSTEM_CACHE_CLEANABLE.into(), Deny),
        (
            classification_id::SYSTEM_CACHE_GUIDANCE.into(),
            GuidanceOnly,
        ),
        (
            classification_id::APP_MANAGED_USER_DATA.into(),
            GuidanceOnly,
        ),
        (classification_id::INSTALLED_APP.into(), GuidanceOnly),
        (classification_id::TRASH_STORE.into(), GuidanceOnly),
        (classification_id::OS_AREA_USER_ITEM.into(), Allow),
        (classification_id::USER_MANAGED.into(), Allow),
        (classification_id::MEMORY_BACKED_TEMP.into(), NoChange),
    ]);
    let release_state = classification_id::ALL
        .iter()
        .map(|id| {
            (
                (*id).to_owned(),
                if RELEASABLE.contains(id) {
                    Protected
                } else {
                    NotReleasable
                },
            )
        })
        .collect();
    RulesetBody {
        class_actions: actions,
        class_release_state: release_state,
        user_rules: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
