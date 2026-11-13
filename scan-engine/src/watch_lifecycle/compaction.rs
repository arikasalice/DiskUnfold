//! Conservative, unit-ordered gap compaction.
use super::model::{Action, Error};
use super::unit::Unit;
use crate::watch_coalescer::{Batch, Gap, GapReason, Trust};
use std::collections::BTreeSet;

impl Unit {
    /// Compacts exactly the oldest closed half when their reason priority is
    /// defined by DLD48. Open gaps are never discarded.
    pub(super) fn compact_for_capacity(&mut self, at: u64) -> Result<Option<Vec<Action>>, Error> {
        self.check_time(at)?;
        if self.generation == 0 {
            return Ok(None);
        }
        let count = self.config.gap_limit / 2;
        if count < 2 {
            return Ok(None);
        }
        let mut closed: Vec<&Gap> = self.gaps.iter().filter(|g| g.end.is_some()).collect();
        closed.sort_by_key(|gap| (gap.end, gap.id));
        if closed.len() < count {
            return Ok(None);
        }
        let selected = &closed[..count];
        let mut priority = 0;
        for gap in selected {
            let Some(rank) = rank(gap.reason) else {
                return Ok(None);
            };
            priority = priority.max(rank);
        }
        let reason = reason(priority).ok_or(Error::GapCapacity)?;
        let start = if selected.iter().any(|g| g.start.is_none()) {
            None
        } else {
            selected.iter().filter_map(|g| g.start).min()
        };
        let end = selected.iter().filter_map(|g| g.end).max();
        let id = self
            .last_gap
            .checked_add(1)
            .ok_or(Error::CounterExhausted)?;
        let summary = Gap {
            id,
            reason,
            path: None,
            start,
            end,
        };
        let replaced_ids: Vec<_> = selected.iter().map(|gap| gap.id).collect();
        let removed: BTreeSet<_> = replaced_ids.iter().copied().collect();
        let (preceding, batch) = if let Some(stream) = self.stream.as_mut() {
            let mut ordered = stream.compacted_gap(&summary, &replaced_ids, at)?;
            let batch = ordered.pop().ok_or(Error::InvalidState)?;
            (ordered, batch)
        } else {
            self.check_sequences(1)?;
            self.check_changes(1)?;
            let mut gaps = vec![summary.clone()];
            gaps.extend(self.gaps.iter().filter(|g| g.end.is_none()).cloned());
            if gaps.len() > self.config.gap_limit {
                return Ok(None);
            }
            let batch = Batch {
                context: self.context(self.generation),
                sequence: self.sequence + 1,
                change_sequence: self.change_sequence + 1,
                replaced_gap_ids: replaced_ids.clone(),
                interval_start: at,
                interval_end: at,
                changes: Vec::new(),
                gaps,
                trust: Trust::Gap,
            };
            (Vec::new(), batch)
        };
        self.gaps.retain(|gap| !removed.contains(&gap.id));
        self.gaps.push(summary);
        self.sequence = batch.sequence;
        self.change_sequence = batch.change_sequence;
        self.last_gap = id;
        self.last_time = at;
        let mut actions: Vec<_> = preceding.into_iter().map(Action::Changes).collect();
        actions.push(Action::Changes(batch));
        Ok(Some(actions))
    }
}
fn rank(reason: GapReason) -> Option<u8> {
    match reason {
        GapReason::BeforeStart => Some(1),
        GapReason::Stopped => Some(2),
        GapReason::Overflow => Some(3),
        GapReason::SignalLost => Some(4),
        GapReason::Unavailable | GapReason::PartialCapability => Some(5),
        GapReason::ConsumerResync => None,
    }
}
fn reason(rank: u8) -> Option<GapReason> {
    match rank {
        1 => Some(GapReason::BeforeStart),
        2 => Some(GapReason::Stopped),
        3 => Some(GapReason::Overflow),
        4 => Some(GapReason::SignalLost),
        5 => Some(GapReason::Unavailable),
        _ => None,
    }
}
