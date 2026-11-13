use du_scan_engine::watch_coalescer::{Gap, GapReason, Key};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn valid(g: &Gap) -> bool {
    g.id != 0
        && g.reason != GapReason::ConsumerResync
        && !matches!((g.start, g.end), (Some(a), Some(b)) if a > b)
}

/// A snapshot cannot contain a gap boundary later than its observation cut.
pub(super) fn valid_at(g: &Gap, observed_at: u64) -> bool {
    valid(g)
        && g.start.is_none_or(|start| start <= observed_at)
        && g.end.is_none_or(|end| end <= observed_at)
}
fn rank(r: GapReason) -> u8 {
    match r {
        GapReason::BeforeStart => 1,
        GapReason::Stopped => 2,
        GapReason::Overflow => 3,
        GapReason::SignalLost => 4,
        GapReason::Unavailable | GapReason::PartialCapability => 5,
        GapReason::ConsumerResync => 6,
    }
}
fn path_covers(a: &Option<Key>, b: &Option<Key>) -> bool {
    match (a, b) {
        (None, _) => true,
        (Some(a), Some(b)) => b.starts_with(a),
        _ => false,
    }
}
fn covers(a: &Gap, b: &Gap) -> bool {
    path_covers(&a.path, &b.path)
        && rank(a.reason) >= rank(b.reason)
        && match (a.start, b.start) {
            (None, _) => true,
            (Some(a), Some(b)) => a <= b,
            _ => false,
        }
        && matches!((a.end, b.end), (Some(a), Some(b)) if a >= b)
}
fn same_or_closed(a: &Gap, b: &Gap) -> bool {
    a == b
        || (a.end.is_none()
            && b.end.is_some()
            && a.id == b.id
            && a.reason == b.reason
            && a.path == b.path
            && a.start == b.start)
}
pub(super) fn apply(
    existing: &BTreeMap<u64, Gap>,
    gaps: &[Gap],
    replaced: &[u64],
) -> Option<BTreeMap<u64, Gap>> {
    let mut next = existing.clone();
    let ids: BTreeSet<_> = replaced.iter().copied().collect();
    if ids.len() != replaced.len() {
        return None;
    }
    let mut incoming = BTreeSet::new();
    if gaps.iter().any(|g| !valid(g) || !incoming.insert(g.id)) {
        return None;
    }
    if !ids.is_empty() {
        if ids.iter().any(|id| incoming.contains(id)) {
            return None;
        }
        let summaries: Vec<_> = gaps
            .iter()
            .filter(|g| !existing.contains_key(&g.id) && g.end.is_some())
            .collect();
        if summaries.len() != 1 {
            return None;
        }
        for id in &ids {
            let old = existing.get(id)?;
            if old.end.is_none() || !covers(summaries[0], old) {
                return None;
            }
        }
        for id in ids {
            next.remove(&id);
        }
    }
    for gap in gaps {
        if next
            .get(&gap.id)
            .is_some_and(|old| !same_or_closed(old, gap))
        {
            return None;
        }
        next.insert(gap.id, gap.clone());
    }
    Some(next)
}
