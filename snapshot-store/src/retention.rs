//! Read-only retention.order revision 1 evaluation.
use std::collections::BTreeSet;

const DAY_MS: i128 = 86_400_000;
const MIN_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024 * 1024;

/// User-selected limits. `None` leaves a limit unset; the default never prunes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    pub revision: u64,
    pub automatic: bool,
    pub count: Option<u32>,
    pub bytes: Option<u64>,
    pub age_days: Option<u32>,
}

/// One normal index row with current protection facts, supplied by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub id: [u8; 16],
    pub scope_label: Vec<u8>,
    /// Missing or unparseable observations are represented by `None`.
    pub observed_ms: Option<i64>,
    pub bytes: u64,
    pub pinned: bool,
    pub catalog_referenced: bool,
}

/// Exclusive classification in the published priority order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Classification {
    Pinned,
    Catalog,
    UnknownTime,
    Candidate,
}

/// One protected or excluded row, preserving the classification reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classified {
    pub id: [u8; 16],
    pub reason: Classification,
    pub bytes: u64,
}

/// Count and total file bytes for a set of snapshots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub count: u128,
    pub bytes: u128,
}

/// Totals for one protection or exclusion reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReasonTotals {
    pub reason: Classification,
    pub totals: Totals,
}

/// Remaining amount over each configured limit, including protected rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Excess {
    pub count: u128,
    pub bytes: u128,
    pub expired: u64,
}

impl Excess {
    fn any(self) -> bool {
        self.count != 0 || self.bytes != 0 || self.expired != 0
    }
}

/// Current status before any cleanup. An active cleanup is tracked elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NoLimits,
    Within,
    OverLimit,
}

/// A deterministic preview; producing it never deletes or modifies snapshots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub rule_id: &'static str,
    pub rule_revision: u32,
    pub policy_revision: u64,
    pub evaluated_at_ms: i64,
    pub status: Status,
    pub current_excess: Excess,
    pub remaining_excess: Excess,
    pub planned: Vec<[u8; 16]>,
    pub planned_totals: Totals,
    pub protected_or_excluded: Vec<Classified>,
    pub reason_totals: Vec<ReasonTotals>,
    /// Rows whose age could not be determined when an age limit is configured.
    pub unknown_age_count: u128,
    pub protected_shortfall: bool,
    /// The caller may submit cleanup only when this is true.
    pub should_submit_prune: bool,
}

/// Invalid limits, duplicate identities, or arithmetic overflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidPolicy,
    DuplicateId,
    Overflow,
}

fn validate(policy: &Policy) -> Result<(), Error> {
    if policy.count.is_some_and(|n| !(1..=10_000).contains(&n))
        || policy
            .bytes
            .is_some_and(|n| !(MIN_BYTES..=MAX_BYTES).contains(&n))
        || policy.age_days.is_some_and(|n| !(1..=3_650).contains(&n))
    {
        return Err(Error::InvalidPolicy);
    }
    Ok(())
}

fn classify(row: &Snapshot, now: i64) -> Classification {
    if row.pinned {
        Classification::Pinned
    } else if row.catalog_referenced {
        Classification::Catalog
    } else if row.observed_ms.is_none_or(|time| time > now) {
        Classification::UnknownTime
    } else {
        Classification::Candidate
    }
}

fn expired(row: &Snapshot, now: i64, days: Option<u32>) -> bool {
    match (row.observed_ms, days) {
        (Some(time), Some(days)) if time <= now => {
            i128::from(now) - i128::from(time) > i128::from(days) * DAY_MS
        }
        _ => false,
    }
}

fn excess(policy: &Policy, count: u128, bytes: u128, expired: u64) -> Excess {
    Excess {
        count: policy
            .count
            .map_or(0, |limit| count.saturating_sub(u128::from(limit))),
        bytes: policy
            .bytes
            .map_or(0, |limit| bytes.saturating_sub(u128::from(limit))),
        expired,
    }
}

/// Evaluates limits using one supplied UTC instant, independent of input row order.
///
/// Unknown observation times are excluded from age decisions, but remain in
/// count and byte totals. Preview candidates are returned even if automatic
/// cleanup is disabled; `should_submit_prune` remains false in that case.
/// Fingerprinting, persistence and transaction-time protection rechecks are
/// intentionally outside this pure evaluation.
pub fn evaluate(policy: &Policy, now_ms: i64, rows: &[Snapshot]) -> Result<Decision, Error> {
    validate(policy)?;
    let mut ids = BTreeSet::new();
    let mut candidates = Vec::new();
    let mut excluded = Vec::new();
    let mut total_bytes = 0u128;
    let mut expired_count = 0u64;
    let mut unknown_age_count = 0u128;
    for row in rows {
        if !ids.insert(row.id) {
            return Err(Error::DuplicateId);
        }
        total_bytes = total_bytes
            .checked_add(u128::from(row.bytes))
            .ok_or(Error::Overflow)?;
        if expired(row, now_ms, policy.age_days) {
            expired_count = expired_count.checked_add(1).ok_or(Error::Overflow)?;
        }
        if policy.age_days.is_some() && row.observed_ms.is_none_or(|time| time > now_ms) {
            unknown_age_count += 1;
        }
        let reason = classify(row, now_ms);
        if reason == Classification::Candidate {
            candidates.push(row);
        } else {
            excluded.push(Classified {
                id: row.id,
                reason,
                bytes: row.bytes,
            });
        }
    }
    candidates.sort_by(|a, b| {
        a.observed_ms
            .cmp(&b.observed_ms)
            .then_with(|| a.scope_label.cmp(&b.scope_label))
            .then_with(|| a.id.cmp(&b.id))
    });
    excluded.sort_by_key(|row| row.id);
    let mut count = rows.len() as u128;
    let current = excess(policy, count, total_bytes, expired_count);
    let mut planned = Vec::new();
    let mut planned_totals = Totals::default();
    for row in candidates {
        let age = expired(row, now_ms, policy.age_days);
        let remaining = excess(policy, count, total_bytes, expired_count);
        if remaining.count == 0 && remaining.bytes == 0 && !age {
            break;
        }
        planned.push(row.id);
        planned_totals.count += 1;
        planned_totals.bytes += u128::from(row.bytes);
        count -= 1;
        total_bytes -= u128::from(row.bytes);
        if age {
            expired_count -= 1;
        }
    }
    let remaining = excess(policy, count, total_bytes, expired_count);
    let no_limits = policy.count.is_none() && policy.bytes.is_none() && policy.age_days.is_none();
    let status = if no_limits {
        Status::NoLimits
    } else if current.any() {
        Status::OverLimit
    } else {
        Status::Within
    };
    let reason_totals = [
        Classification::Pinned,
        Classification::Catalog,
        Classification::UnknownTime,
    ]
    .into_iter()
    .map(|reason| {
        let totals = excluded.iter().filter(|row| row.reason == reason).fold(
            Totals::default(),
            |mut total, row| {
                total.count += 1;
                total.bytes += u128::from(row.bytes);
                total
            },
        );
        ReasonTotals { reason, totals }
    })
    .collect();
    Ok(Decision {
        rule_id: "retention.order",
        rule_revision: 1,
        policy_revision: policy.revision,
        evaluated_at_ms: now_ms,
        status,
        current_excess: current,
        remaining_excess: remaining,
        protected_shortfall: remaining.any(),
        should_submit_prune: policy.automatic && current.any() && !planned.is_empty(),
        planned,
        planned_totals,
        protected_or_excluded: excluded,
        reason_totals,
        unknown_age_count,
    })
}

#[cfg(test)]
mod tests;
