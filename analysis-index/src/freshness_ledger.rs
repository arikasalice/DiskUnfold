//! Session-memory freshness ledgers, one per scope version, with one row per scope root.
//!
//! A ledger changes under the result-store lock and returns the notices to publish after
//! the lock is released. Row states reuse the transition rules in [`crate::freshness`].
//! Nothing watches files yet, so a root observed in full settles as `unwatched`.
//!
//! # Work under the store lock
//!
//! Query admissions wait on the result-store lock, so everything in this module may do
//! only O(number of roots) work. The ledger therefore never sees a result's items or gaps:
//! it takes a [`LedgerVersion`], which carries the root count and the per-root outcomes
//! that the result's builder computed outside the lock (see [`crate::freshness_outcomes`]).
//! Do not add a path from here to the items, gaps or any other per-item data.
use crate::{
    freshness::{Event, Reobservation, Settled, State, Summary, transition},
    freshness_outcomes::RootOutcome,
};
use du_core_contracts::{
    Id, Timestamp,
    result_freshness::{
        FreshnessChanged, FreshnessCounts, FreshnessRange, FreshnessState, FreshnessSummary,
        MAX_FRESHNESS_RANGES, ResultFreshnessChanged, ResultFreshnessNotice,
    },
};
use std::collections::{HashMap, HashSet, VecDeque};

/// The part of a result version the ledger reads, borrowed from the version.
///
/// Building it costs O(1). The outcomes are present when the version was sealed by a
/// store that tracks freshness. A version without them is settled as if no root was
/// processed and none failed: each refreshing row returns to its earlier state, or becomes
/// `unverifiable` without a reason key when it had none. The ledger never derives outcomes
/// itself, because that needs a walk over the items.
pub(crate) struct LedgerVersion<'a> {
    pub(crate) version_id: &'a Id,
    pub(crate) scope_version_id: &'a Id,
    pub(crate) root_count: usize,
    pub(crate) observed_at: Option<&'a Timestamp>,
    pub(crate) outcomes: Option<&'a [RootOutcome]>,
}

/// How many sequences of removed ledgers are remembered.
///
/// A scope version that becomes current again continues its sequence, so a receiver that
/// dropped the removed ledger never sees the sequence go backwards. The list is bounded;
/// a scope version evicted from it restarts at 1. A receiver forgets the last sequence of
/// a scope version when it receives that scope version's removal event, and accepts the
/// next event of every scope version after `job.resync_required`, so it handles the
/// restart as a first event.
const RETIRED_LIMIT: usize = 64;

/// Sequences stay below 2^53 so that they are exact in a JSON number on the GUI side.
const SEQUENCE_LIMIT: u64 = 1_u64 << 53;

/// The representative state of a summary without rows. A receiver must not assume a
/// state for a ledger that is gone, so it is the least certain one.
const NO_ROWS_REPRESENTATIVE: &str = FreshnessState::UNVERIFIABLE;

/// A settled state with the reason that explains it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rested {
    state: Settled,
    reason: Option<String>,
}

/// One root's row. A refreshing row keeps its state before the refresh, or none when the
/// root had no row before, in which case an abort removes the row.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    Settled(Rested),
    Refreshing { before: Option<Rested> },
}

impl Row {
    fn new() -> Self {
        Self::Refreshing { before: None }
    }

    fn name(&self) -> &'static str {
        match self.summary() {
            Summary::Observed => FreshnessState::OBSERVED,
            Summary::Unwatched => FreshnessState::UNWATCHED,
            Summary::Refreshing => FreshnessState::REFRESHING,
            Summary::ChangeNotified => FreshnessState::CHANGE_NOTIFIED,
            Summary::GapUnknown => FreshnessState::GAP_UNKNOWN,
            Summary::Unverifiable => FreshnessState::UNVERIFIABLE,
        }
    }

    fn summary(&self) -> Summary {
        match self {
            Self::Refreshing { .. } => Summary::Refreshing,
            Self::Settled(rested) => match rested.state {
                Settled::Observed => Summary::Observed,
                Settled::Unwatched => Summary::Unwatched,
                Settled::ChangeNotified => Summary::ChangeNotified,
                Settled::GapUnknown => Summary::GapUnknown,
                Settled::Unverifiable => Summary::Unverifiable,
            },
        }
    }

    /// What a receiver can see; a change of anything else raises no sequence.
    fn wire(&self) -> (&'static str, Option<&str>) {
        let reason = match self {
            Self::Settled(rested) => rested.reason.as_deref(),
            Self::Refreshing { .. } => None,
        };
        (self.name(), reason)
    }

    fn begin_refresh(&mut self) {
        if let Self::Settled(rested) = self {
            *self = Self::Refreshing {
                before: Some(rested.clone()),
            };
        }
    }

    fn restored(&self) -> Option<Self> {
        match self {
            Self::Settled(_) => Some(self.clone()),
            Self::Refreshing { before } => before.clone().map(Self::Settled),
        }
    }

    /// Settles a refreshing row from a completed observation.
    ///
    /// A root the observation did not process keeps its state before the refresh, and is
    /// `unverifiable` without a reason when it had none.
    fn settle(&mut self, outcome: &RootOutcome) {
        let Self::Refreshing { before } = self else {
            return;
        };
        let previous = before.as_ref().map_or(Settled::Unverifiable, |b| b.state);
        let evidence = Reobservation {
            failed: outcome.failed.is_some(),
            processed: outcome.processed,
            open_gap: false,
            last_gap_end: None,
            last_notification: None,
            started_at: 0,
            watching: false,
        };
        let Ok(State::Settled(state)) = transition(
            State::Refreshing { previous },
            Event::CompleteRefresh(evidence),
        ) else {
            return;
        };
        let reason = if outcome.failed.is_some() {
            outcome.failed.clone()
        } else if outcome.processed {
            None
        } else {
            before.as_ref().and_then(|b| b.reason.clone())
        };
        *self = Self::Settled(Rested { state, reason });
    }
}

struct Ledger {
    lineage: Id,
    rows: Vec<Row>,
    sequence: u64,
}

impl Ledger {
    fn summary(&self) -> FreshnessSummary {
        let mut counts = FreshnessCounts::default();
        for row in &self.rows {
            let slot = match row.summary() {
                Summary::Observed => &mut counts.observed,
                Summary::Unwatched => &mut counts.unwatched,
                Summary::ChangeNotified => &mut counts.change_notified,
                Summary::GapUnknown => &mut counts.gap_unknown,
                Summary::Refreshing => &mut counts.refreshing,
                Summary::Unverifiable => &mut counts.unverifiable,
            };
            *slot += 1;
        }
        let worst = self.rows.iter().map(Row::summary).max();
        let representative = self
            .rows
            .iter()
            .find(|row| Some(row.summary()) == worst)
            .map_or(NO_ROWS_REPRESENTATIVE, Row::name);
        FreshnessSummary {
            representative: FreshnessState::new(representative),
            counts,
        }
    }
}

/// All ledgers of one result store.
#[derive(Default)]
pub(crate) struct Ledgers {
    by_scope: HashMap<Id, Ledger>,
    retired: VecDeque<(Id, u64)>,
}

impl Ledgers {
    /// Applies a publication of `result` by a build of `lineage`.
    ///
    /// The first publication of a build starts a refresh of every root; sealing settles
    /// every root from the result's stored outcomes. Other publications change nothing.
    /// Runs under the store lock: O(roots) only.
    pub(crate) fn publish(
        &mut self,
        lineage: &Id,
        result: &LedgerVersion<'_>,
        first: bool,
        finish: bool,
    ) -> Option<ResultFreshnessNotice> {
        self.update(lineage, result, |rows| {
            if first {
                rows.iter_mut().for_each(Row::begin_refresh);
            }
            if finish {
                settle_all(rows, result.outcomes);
            }
            finish
        })
    }

    /// Returns the rows a withdrawn build refreshed to their earlier states.
    ///
    /// `baseline` is the version the withdrawal made current again. A ledger that cannot be
    /// restored, such as one for a scope version the baseline had lost, is derived again
    /// from the baseline's stored outcomes. Runs under the store lock: O(roots) only.
    pub(crate) fn abort(
        &mut self,
        lineage: &Id,
        build_scope: &Id,
        baseline: &LedgerVersion<'_>,
    ) -> Option<ResultFreshnessNotice> {
        let scope = baseline.scope_version_id;
        if self.by_scope.contains_key(scope) && scope != build_scope {
            return None;
        }
        self.update(lineage, baseline, |rows| {
            let restored: Option<Vec<Row>> = rows.iter().map(Row::restored).collect();
            if let Some(restored) = restored {
                *rows = restored;
                return false;
            }
            rows.iter_mut().for_each(|row| *row = Row::new());
            settle_all(rows, baseline.outcomes);
            true
        })
    }

    /// Removes the ledgers of scope versions that no lineage's current version uses.
    ///
    /// Each removal returns a notice with no current version, so receivers drop what they
    /// kept. This keeps the number of ledgers at most the number of current versions.
    pub(crate) fn prune(&mut self, live: &HashSet<&Id>) -> Vec<ResultFreshnessNotice> {
        let mut gone: Vec<Id> = self
            .by_scope
            .keys()
            .filter(|scope| !live.contains(scope))
            .cloned()
            .collect();
        gone.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let mut notices = Vec::new();
        for scope in gone {
            let Some(ledger) = self.by_scope.remove(&scope) else {
                continue;
            };
            let next = ledger.sequence.saturating_add(1).min(SEQUENCE_LIMIT - 1);
            if self.retired.len() == RETIRED_LIMIT {
                self.retired.pop_front();
            }
            self.retired.push_back((scope.clone(), next));
            notices.push(ResultFreshnessNotice {
                result_version: None,
                body: ResultFreshnessChanged {
                    lineage_id: ledger.lineage,
                    scope_version_id: scope,
                    sequence: next,
                    summary: FreshnessSummary {
                        representative: FreshnessState::new(NO_ROWS_REPRESENTATIVE),
                        counts: FreshnessCounts::default(),
                    },
                    changed: FreshnessChanged::Requery,
                    observed_at: None,
                },
            });
        }
        notices
    }

    /// Edits the ledger of `current`'s scope version and reports what changed.
    ///
    /// `edit` returns whether the change comes from an observation of `current`. Everything
    /// here, including `edit`, must stay O(roots): the caller holds the store lock.
    fn update(
        &mut self,
        lineage: &Id,
        current: &LedgerVersion<'_>,
        edit: impl FnOnce(&mut Vec<Row>) -> bool,
    ) -> Option<ResultFreshnessNotice> {
        let scope = current.scope_version_id;
        let count = current.root_count;
        if count == 0 {
            return None;
        }
        let existed = self.by_scope.contains_key(scope);
        let floor = self.take_retired(scope);
        let ledger = self
            .by_scope
            .entry(scope.clone())
            .or_insert_with(|| Ledger {
                lineage: lineage.clone(),
                rows: Vec::new(),
                sequence: floor,
            });
        let before: Vec<(&'static str, Option<String>)> = ledger
            .rows
            .iter()
            .map(|row| (row.wire().0, row.wire().1.map(str::to_owned)))
            .collect();
        let shrunk = ledger.rows.len() > count;
        ledger.rows.resize(count, Row::new());
        let observed = edit(&mut ledger.rows);
        ledger.lineage = lineage.clone();
        let changed: Vec<usize> = ledger
            .rows
            .iter()
            .enumerate()
            .filter(|(index, row)| {
                !existed
                    || before
                        .get(*index)
                        .is_none_or(|old| old.0 != row.wire().0 || old.1.as_deref() != row.wire().1)
            })
            .map(|(index, _)| index)
            .collect();
        if changed.is_empty() && !shrunk {
            return None;
        }
        // When the next sequence would reach the limit, the rows above already hold the
        // change, but `?` returns no notice and the sequence does not move. The ledger then
        // differs from what receivers last saw, and every later change of this scope version
        // is applied the same way, silently, because the next diff starts from the changed
        // rows. Only a removal still sends a notice, and it reuses the last sequence (see
        // `prune`). Reaching this takes 2^53 changes; the behaviour is left as it is.
        let sequence = ledger
            .sequence
            .checked_add(1)
            .filter(|next| *next < SEQUENCE_LIMIT)?;
        ledger.sequence = sequence;
        let changed = if shrunk || changed.len() > MAX_FRESHNESS_RANGES {
            FreshnessChanged::Requery
        } else {
            FreshnessChanged::Ranges {
                ranges: changed
                    .into_iter()
                    .filter_map(|index| range(index, ledger.rows.get(index)?))
                    .collect(),
            }
        };
        Some(ResultFreshnessNotice {
            result_version: Some(current.version_id.clone()),
            body: ResultFreshnessChanged {
                lineage_id: lineage.clone(),
                scope_version_id: scope.clone(),
                sequence,
                summary: ledger.summary(),
                changed,
                observed_at: observed.then(|| current.observed_at.cloned()).flatten(),
            },
        })
    }

    fn take_retired(&mut self, scope: &Id) -> u64 {
        let position = self.retired.iter().position(|(id, _)| id == scope);
        position
            .and_then(|index| self.retired.remove(index))
            .map_or(0, |(_, sequence)| sequence)
    }
}

fn range(index: usize, row: &Row) -> Option<FreshnessRange> {
    let (state, reason) = row.wire();
    Some(FreshnessRange {
        root_index: u32::try_from(index).ok()?,
        state: FreshnessState::new(state),
        reason_key: reason.map(str::to_owned),
    })
}

/// Settles every refreshing row from stored outcomes, or as not processed without them.
fn settle_all(rows: &mut [Row], stored: Option<&[RootOutcome]>) {
    let idle = RootOutcome::default();
    for (index, row) in rows.iter_mut().enumerate() {
        row.settle(stored.and_then(|all| all.get(index)).unwrap_or(&idle));
    }
}

#[cfg(test)]
mod tests;
