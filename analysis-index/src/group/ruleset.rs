//! F36 virtual-group ruleset review, normalization and fingerprint.
//!
//! These functions are pure: they decide whether a draft ruleset may be stored
//! and what the next stored version is. Persistence goes through the
//! `GroupRulesStore` port, called by the engine use case.
use std::collections::BTreeSet;

use du_core_contracts::Id;

use crate::query::{self, Condition, Predicate, Registry};

/// Version of the stored ruleset layout covered by the fingerprint.
pub const RULESET_FORMAT_VERSION: u32 = 1;
/// Revision of the F04 condition model used by stored conditions.
pub const CONDITION_MODEL_VERSION: u32 = 1;
const DOMAIN: &[u8] = b"diskunfold.group.ruleset.v1\0";

/// Provisional review limits (F36 DLD Q3), kept in one place until measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleLimits {
    pub max_rules: usize,
    pub max_name_chars: usize,
}

impl Default for RuleLimits {
    fn default() -> Self {
        Self {
            max_rules: 64,
            max_name_chars: 64,
        }
    }
}

/// A rule as edited by the user. A missing group ID creates a new group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleDraft {
    pub group_id: Option<Id>,
    pub display_name: String,
    pub condition: Condition,
}

/// A reviewed rule; its display order is its position in the ruleset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub group_id: Id,
    pub display_name: String,
    /// Normalized condition.
    pub condition: Condition,
    /// Leaf predicate strategies, sorted and unique.
    pub predicates: Vec<(String, u32)>,
}

/// One stored ruleset version. Version and revision are zero before the first save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ruleset {
    pub version_no: u32,
    pub revision: u32,
    pub fingerprint: [u8; 32],
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub rules: Vec<Rule>,
}

impl Ruleset {
    /// The implicit ruleset before anything was stored.
    pub fn empty() -> Self {
        Self {
            version_no: 0,
            revision: 0,
            fingerprint: fingerprint(&[]),
            created_at_ms: None,
            updated_at_ms: None,
            rules: Vec::new(),
        }
    }
}

/// Review result of a stored rule when the ruleset is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleReview {
    Ok,
    /// A predicate is not registered; the rule is kept and every file is undecidable.
    PredicateUnregistered,
}

/// The first failed review check of one draft rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Issue {
    pub index: usize,
    pub reason_key: &'static str,
}

/// What a save request should do with the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SavePlan {
    /// The content fingerprint equals the current version; nothing is written.
    Unchanged,
    /// At least one rule failed review; nothing is written.
    Rejected(Vec<Issue>),
    /// Replace the stored ruleset with this next version.
    Write(Ruleset),
}

/// Trims surrounding white space from a display name.
///
/// Callers that receive text from an IPC boundary pass it in NFC; this function
/// does not change the Unicode normalization form.
pub fn normalize_name(name: &str) -> String {
    name.trim().to_owned()
}

/// Reviews opened rules without replacing an unregistered predicate.
pub fn review_stored(registry: &Registry, rules: &[Rule]) -> Vec<RuleReview> {
    rules
        .iter()
        .map(
            |rule| match query::plan::prepare(registry, &rule.condition, 0) {
                Err(query::Error::UnknownPredicate) => RuleReview::PredicateUnregistered,
                _ => RuleReview::Ok,
            },
        )
        .collect()
}

/// Reviews `drafts` against `current` and returns the storage action.
///
/// Every rule is checked in a fixed order (condition model, predicate
/// registration, name, duplicate name, rule limit, group identity); each
/// rejected rule reports its first failing check. Group IDs are kept, and
/// `new_id` is called only for new rules of a ruleset that will be written.
pub fn plan_save(
    current: &Ruleset,
    drafts: Vec<RuleDraft>,
    registry: &Registry,
    limits: RuleLimits,
    now_ms: i64,
    new_id: &mut dyn FnMut() -> Id,
) -> SavePlan {
    let issues = review_drafts(current, &drafts, registry, limits);
    if !issues.is_empty() {
        return SavePlan::Rejected(issues);
    }
    let mut rules = Vec::with_capacity(drafts.len());
    let mut pending = Vec::new();
    for draft in drafts {
        let condition = draft.condition.normalize();
        pending.push(draft.group_id.is_none());
        rules.push(Rule {
            group_id: draft.group_id.unwrap_or_else(|| Id::from_v7_bytes([0; 16])),
            display_name: normalize_name(&draft.display_name),
            predicates: strategies(&condition),
            condition,
        });
    }
    let fingerprint = fingerprint(&rules);
    if fingerprint == current.fingerprint {
        return SavePlan::Unchanged;
    }
    for (rule, new) in rules.iter_mut().zip(pending) {
        if new {
            rule.group_id = new_id();
        }
    }
    SavePlan::Write(Ruleset {
        version_no: current.version_no.saturating_add(1),
        revision: current.revision.saturating_add(1),
        fingerprint,
        created_at_ms: current.created_at_ms.or(Some(now_ms)),
        updated_at_ms: Some(now_ms),
        rules,
    })
}

fn review_drafts(
    current: &Ruleset,
    drafts: &[RuleDraft],
    registry: &Registry,
    limits: RuleLimits,
) -> Vec<Issue> {
    let mut names = BTreeSet::new();
    let mut groups = Vec::new();
    let mut issues = Vec::new();
    for (index, draft) in drafts.iter().enumerate() {
        let name = normalize_name(&draft.display_name);
        let reason = review_condition(registry, &draft.condition)
            .or_else(|| {
                let chars = name.chars().count();
                // A name outside 1..=max_name_chars breaks the name length limit (DLD Q3).
                (chars == 0 || chars > limits.max_name_chars).then_some("group.rule_limit")
            })
            .or_else(|| (!names.insert(name.to_lowercase())).then_some("group.name_duplicate"))
            .or_else(|| (index >= limits.max_rules).then_some("group.rule_limit"))
            .or_else(|| review_group(current, &mut groups, draft.group_id.as_ref()));
        if let Some(reason_key) = reason {
            issues.push(Issue { index, reason_key });
        }
    }
    issues
}

fn review_condition(registry: &Registry, condition: &Condition) -> Option<&'static str> {
    if query::validate_condition(condition).is_err() {
        return Some("query.syntax_invalid");
    }
    let normalized = condition.clone().normalize();
    match query::plan::prepare(registry, &normalized, 0) {
        Ok(_) => None,
        Err(query::Error::UnknownPredicate) => Some("predicate.unknown"),
        Err(_) => Some("query.syntax_invalid"),
    }
}

fn review_group(
    current: &Ruleset,
    seen: &mut Vec<Id>,
    group_id: Option<&Id>,
) -> Option<&'static str> {
    let id = group_id?;
    let known = current.rules.iter().any(|rule| &rule.group_id == id);
    let repeated = seen.contains(id);
    seen.push(id.clone());
    (!known || repeated).then_some("group.unknown")
}

/// Leaf predicate strategies of a condition, sorted and unique.
pub fn strategies(condition: &Condition) -> Vec<(String, u32)> {
    let mut found = BTreeSet::new();
    collect(condition, &mut found);
    found.into_iter().collect()
}

fn collect(condition: &Condition, found: &mut BTreeSet<(String, u32)>) {
    match condition {
        Condition::Predicate(predicate) => {
            let (id, revision) = predicate.strategy();
            found.insert((id.to_owned(), revision));
        }
        Condition::All(children) | Condition::Any(children) => {
            children.iter().for_each(|child| collect(child, found));
        }
        Condition::Not(child) => collect(child, found),
    }
}

/// BLAKE3 digest of the format version and, in display order, each rule's
/// name, canonical condition bytes, condition model version and predicates.
/// Group IDs are excluded so that identical content has the same fingerprint.
pub fn fingerprint(rules: &[Rule]) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(DOMAIN);
    hash.update(&RULESET_FORMAT_VERSION.to_le_bytes());
    hash.update(&(rules.len() as u64).to_le_bytes());
    for rule in rules {
        field(&mut hash, rule.display_name.as_bytes());
        field(
            &mut hash,
            &canonical_bytes(&rule.condition.clone().normalize()),
        );
        hash.update(&CONDITION_MODEL_VERSION.to_le_bytes());
        hash.update(&(rule.predicates.len() as u64).to_le_bytes());
        for (id, revision) in &rule.predicates {
            field(&mut hash, id.as_bytes());
            hash.update(&revision.to_le_bytes());
        }
    }
    *hash.finalize().as_bytes()
}

fn field(hash: &mut blake3::Hasher, bytes: &[u8]) {
    hash.update(&(bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend((bytes.len() as u64).to_le_bytes());
    out.extend(bytes);
}

/// Canonical bytes of a normalized condition; sibling order does not matter.
fn canonical_bytes(condition: &Condition) -> Vec<u8> {
    let mut out = Vec::new();
    match condition {
        Condition::All(children) | Condition::Any(children) => {
            out.push(if matches!(condition, Condition::All(_)) {
                1
            } else {
                2
            });
            let mut parts: Vec<_> = children.iter().map(canonical_bytes).collect();
            parts.sort();
            out.extend((parts.len() as u64).to_le_bytes());
            parts.into_iter().for_each(|part| out.extend(part));
        }
        Condition::Not(child) => {
            out.push(3);
            out.extend(canonical_bytes(child));
        }
        Condition::Predicate(predicate) => {
            out.push(4);
            predicate_bytes(&mut out, predicate);
        }
    }
    out
}

fn predicate_bytes(out: &mut Vec<u8>, predicate: &Predicate) {
    use query::{Basis, Boundary, Kind, NameEncoding, NameTest};
    match predicate {
        Predicate::Name(test, encoding) => {
            let (kind, value) = match test {
                NameTest::Equal(v) => (1, v),
                NameTest::Contains(v) => (2, v),
                NameTest::StartsWith(v) => (3, v),
                NameTest::EndsWith(v) => (4, v),
            };
            out.extend([1, kind]);
            push_bytes(out, value);
            out.push(match encoding {
                NameEncoding::Bytes => 1,
                NameEncoding::WindowsUtf16Le => 2,
            });
        }
        Predicate::Kind(kind) => out.extend([
            2,
            match kind {
                Kind::File => 1,
                Kind::Directory => 2,
                Kind::Link => 3,
            },
        ]),
        Predicate::Size(basis, boundary) => {
            let (kind, value) = match boundary {
                Boundary::AtLeast(v) => (1, v),
                Boundary::GreaterThan(v) => (2, v),
                Boundary::AtMost(v) => (3, v),
                Boundary::LessThan(v) => (4, v),
            };
            let basis = match basis {
                Basis::Logical => 1,
                Basis::Allocated => 2,
            };
            out.extend([3, basis, kind]);
            out.extend(value.to_le_bytes());
        }
        Predicate::Registered {
            id,
            revision,
            operand,
        } => {
            out.push(4);
            push_bytes(out, id.as_bytes());
            out.extend(revision.to_le_bytes());
            push_bytes(out, operand);
        }
    }
}

#[cfg(test)]
mod tests;
