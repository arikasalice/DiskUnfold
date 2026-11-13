//! Bounded F16 role decisions over immutable item observations.
//!
//! This subset does not load bundles or evaluate path conditions. A location
//! condition remains unavailable until the owning location contract supplies
//! independently verified evidence. No result is a protection decision.
use du_core_contracts::result::ResultItemKind;
use std::collections::{BTreeMap, BTreeSet};

/// The result-version item facts used by this subset of the rule interpreter.
pub struct Observation<'a> {
    pub kind: ResultItemKind,
    /// Complete, directly observed child names; `None` means unavailable.
    pub marker_children: Option<&'a [&'a [u8]]>,
}

/// Supported conditions in this subset. Names are compared as observed bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Condition<'a> {
    Kind(ResultItemKind),
    MarkerChild(&'a [u8]),
    /// A location condition that cannot be evaluated by this subset.
    LocationUnavailable,
}

/// Role values are descriptions, never authorization or protection levels.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Role {
    OsManaged,
    AppManaged,
    UserData,
    Cache,
    Temp,
    Log,
    Trash,
}

/// A declarative rule. Data provenance and bundle integrity are checked by the
/// bundle owner before supplying rules; this evaluator does not perform loading.
pub struct Rule<'a> {
    pub id: &'a str,
    pub priority: i32,
    pub role: Role,
    pub may_confirm: bool,
    pub conditions: &'a [Condition<'a>],
    pub covers: &'a [&'a str],
}

/// Result of one observed condition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Evidence {
    Satisfied,
    NotSatisfied,
    Unavailable,
}

/// A candidate and the facts actually evaluated for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Candidate<'a> {
    pub id: &'a str,
    pub role: Role,
    pub evidence: Vec<Evidence>,
}

/// A conservative role verdict, separate from protection or action decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Confirmed,
    Estimated,
    Conflict,
    Unknown,
    Unavailable,
}

/// Deterministic evaluation and its candidate evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision<'a> {
    pub verdict: Verdict,
    pub selected: Option<&'a str>,
    pub candidates: Vec<Candidate<'a>>,
    pub location_unverified: bool,
}

/// An invalid rule set is rejected in full.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidRules;

/// Rules whose structural graph was validated once at construction.
///
/// Bundle hash, provenance and closed-format validation remain the bundle
/// owner's responsibility; structural validation alone is not a bundle loader.
pub struct RuleSet<'a> {
    rules: &'a [Rule<'a>],
    order: Vec<usize>,
    cover_order: Vec<usize>,
}

impl<'a> RuleSet<'a> {
    /// Validate the full structural graph and retain deterministic evaluation and coverage orders.
    ///
    /// # Errors
    /// Returns `InvalidRules` for malformed IDs, conditions or coverage graph.
    pub fn new(rules: &'a [Rule<'a>]) -> Result<Self, InvalidRules> {
        let cover_order = validate(rules)?;
        let mut order: Vec<_> = (0..rules.len()).collect();
        order.sort_by(|&a, &b| {
            rules[b]
                .priority
                .cmp(&rules[a].priority)
                .then_with(|| rules[a].id.cmp(rules[b].id))
        });
        Ok(Self {
            rules,
            order,
            cover_order,
        })
    }

    /// Evaluate one immutable observation using the already checked graph.
    #[must_use]
    pub fn evaluate(&self, facts: &Observation<'_>) -> Decision<'a> {
        let mut candidates = Vec::new();
        for &index in &self.order {
            let rule = &self.rules[index];
            let evidence: Vec<_> = rule
                .conditions
                .iter()
                .map(|condition| condition.evaluate(facts))
                .collect();
            if !evidence.contains(&Evidence::NotSatisfied) {
                candidates.push((rule, evidence));
            }
        }
        decide(candidates, self.rules, &self.cover_order)
    }
}

/// Evaluate an available rule set, or return unavailable for a failed load.
///
/// A missing ruleset is distinct from a valid set with no matching rules.
pub fn evaluate_bundle<'a>(rules: Option<&RuleSet<'a>>, facts: &Observation<'_>) -> Decision<'a> {
    match rules {
        Some(rules) => rules.evaluate(facts),
        None => Decision {
            verdict: Verdict::Unavailable,
            selected: None,
            candidates: Vec::new(),
            location_unverified: false,
        },
    }
}

impl Condition<'_> {
    fn evaluate(self, facts: &Observation<'_>) -> Evidence {
        let result = match self {
            Self::Kind(_)
                if matches!(
                    facts.kind,
                    ResultItemKind::Unknown | ResultItemKind::UnknownStructure
                ) =>
            {
                return Evidence::Unavailable;
            }
            Self::Kind(expected) => facts.kind == expected,
            Self::MarkerChild(name) => match facts.marker_children {
                Some(children) => children.contains(&name),
                None => return Evidence::Unavailable,
            },
            Self::LocationUnavailable => return Evidence::Unavailable,
        };
        if result {
            Evidence::Satisfied
        } else {
            Evidence::NotSatisfied
        }
    }
}

/// Resolves explicit coverage in validated topological order without transitive expansion.
fn decide<'a>(
    candidates: Vec<(&'a Rule<'a>, Vec<Evidence>)>,
    rules: &'a [Rule<'a>],
    cover_order: &[usize],
) -> Decision<'a> {
    let location_unverified = candidates.iter().any(|(rule, values)| {
        rule.conditions
            .iter()
            .zip(values)
            .any(|(condition, value)| {
                matches!(condition, Condition::LocationUnavailable)
                    && *value == Evidence::Unavailable
            })
    });
    let fully_observed = candidates
        .iter()
        .any(|(_, values)| !values.contains(&Evidence::Unavailable));
    let active: BTreeMap<_, _> = candidates
        .iter()
        .map(|(rule, evidence)| (rule.id, evidence))
        .collect();
    let mut covered = BTreeSet::new();
    for &index in cover_order {
        let rule = &rules[index];
        if covered.contains(rule.id) {
            continue;
        }
        if active
            .get(rule.id)
            .is_some_and(|evidence| evidence.iter().all(|value| *value == Evidence::Satisfied))
        {
            covered.extend(
                rule.covers
                    .iter()
                    .copied()
                    .filter(|target| active.contains_key(target)),
            );
        }
    }
    let remaining: Vec<_> = candidates
        .iter()
        .filter(|(rule, _)| !covered.contains(rule.id))
        .collect();
    let roles: BTreeSet<_> = remaining.iter().map(|(rule, _)| rule.role).collect();
    let verdict = if candidates.is_empty() {
        Verdict::Unknown
    } else if !fully_observed && location_unverified {
        Verdict::Unavailable
    } else if roles.len() > 1 {
        Verdict::Conflict
    } else if remaining
        .first()
        .is_some_and(|(rule, values)| rule.may_confirm && !values.contains(&Evidence::Unavailable))
        && candidates
            .iter()
            .all(|(_, values)| !values.contains(&Evidence::Unavailable))
    {
        Verdict::Confirmed
    } else {
        Verdict::Estimated
    };
    let selected = match verdict {
        Verdict::Confirmed | Verdict::Estimated => remaining.first().map(|(rule, _)| rule.id),
        _ => None,
    };
    Decision {
        verdict,
        selected,
        location_unverified,
        candidates: candidates
            .into_iter()
            .map(|(rule, evidence)| Candidate {
                id: rule.id,
                role: rule.role,
                evidence,
            })
            .collect(),
    }
}

fn validate(rules: &[Rule<'_>]) -> Result<Vec<usize>, InvalidRules> {
    let mut ids = BTreeMap::new();
    for (index, rule) in rules.iter().enumerate() {
        if rule.id.is_empty() || rule.conditions.is_empty() || ids.insert(rule.id, index).is_some()
        {
            return Err(InvalidRules);
        }
    }
    let mut edges = vec![Vec::new(); rules.len()];
    let mut incoming = vec![0_usize; rules.len()];
    for (index, rule) in rules.iter().enumerate() {
        for target in rule.covers {
            let Some(&target_index) = ids.get(target) else {
                return Err(InvalidRules);
            };
            if index == target_index {
                return Err(InvalidRules);
            }
            edges[index].push(target_index);
            incoming[target_index] += 1;
        }
    }
    let mut ready: Vec<_> = incoming
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect();
    let mut order = Vec::with_capacity(rules.len());
    while !ready.is_empty() {
        ready.sort_by(|&a, &b| {
            rules[b]
                .priority
                .cmp(&rules[a].priority)
                .then_with(|| rules[a].id.cmp(rules[b].id))
        });
        let mut next = Vec::new();
        for index in ready {
            order.push(index);
            for &target in &edges[index] {
                incoming[target] -= 1;
                if incoming[target] == 0 {
                    next.push(target);
                }
            }
        }
        ready = next;
    }
    if order.len() == rules.len() {
        Ok(order)
    } else {
        Err(InvalidRules)
    }
}

#[cfg(test)]
mod tests;
