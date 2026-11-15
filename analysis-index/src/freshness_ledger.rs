//! Session-memory freshness ledgers, one per scope version, with one row per scope root.
//!
//! A ledger changes under the result-store lock and returns the notices to publish after
//! the lock is released. Row states reuse the transition rules in [`crate::freshness`].
//! Nothing watches files yet, so a root observed in full settles as `unwatched`.
use crate::{
    ResultVersion,
    freshness::{Event, Reobservation, Settled, State, Summary, transition},
};
use du_core_contracts::{
    Id,
    result::ResultCompleteness,
    result_freshness::{
        FreshnessChanged, FreshnessCounts, FreshnessRange, FreshnessState, FreshnessSummary,
        MAX_FRESHNESS_RANGES, ResultFreshnessChanged, ResultFreshnessNotice,
    },
};
use std::collections::{HashMap, HashSet, VecDeque};

/// How many sequences of removed ledgers are remembered.
///
/// A scope version that becomes current again continues its sequence, so a receiver that
/// dropped the removed ledger never sees the sequence go backwards. The list is bounded;
/// a scope version evicted from it restarts at 1. A receiver forgets the last sequence of
/// a scope version when it receives that scope version's removal event, and accepts the
/// next event of every scope version after `job.resync_required`, so it handles the
/// restart as a first event.
const RETIRED_LIMIT: usize = 64;

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

/// What a sealed result says about one root.
struct Outcome {
    /// The reason key of a gap that covers the whole root.
    failed: Option<String>,
    processed: bool,
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
    fn settle(&mut self, outcome: &Outcome) {
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
    /// every root from the result. Other publications change nothing.
    pub(crate) fn publish(
        &mut self,
        lineage: &Id,
        result: &ResultVersion,
        first: bool,
        finish: bool,
    ) -> Option<ResultFreshnessNotice> {
        self.update(lineage, result, |rows| {
            if first {
                rows.iter_mut().for_each(Row::begin_refresh);
            }
            if finish {
                settle_all(rows, result);
            }
            finish
        })
    }

    /// Returns the rows a withdrawn build refreshed to their earlier states.
    ///
    /// `baseline` is the version the withdrawal made current again. A ledger that cannot be
    /// restored, such as one for a scope version the baseline had lost, is derived again
    /// from the baseline's own evidence.
    pub(crate) fn abort(
        &mut self,
        lineage: &Id,
        build_scope: &Id,
        baseline: &ResultVersion,
    ) -> Option<ResultFreshnessNotice> {
        let scope = &baseline.scope_version_id;
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
            settle_all(rows, baseline);
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
    /// `edit` returns whether the change comes from an observation of `current`.
    fn update(
        &mut self,
        lineage: &Id,
        current: &ResultVersion,
        edit: impl FnOnce(&mut Vec<Row>) -> bool,
    ) -> Option<ResultFreshnessNotice> {
        let scope = &current.scope_version_id;
        let count = current.roots.len();
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
                observed_at: observed.then(|| current.observed_at.clone()).flatten(),
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

fn settle_all(rows: &mut [Row], result: &ResultVersion) {
    for (row, outcome) in rows.iter_mut().zip(outcomes(result)) {
        row.settle(&outcome);
    }
}

/// Reads each root's outcome from a result's gaps and items.
///
/// A complete result processed every root it has no whole-root gap for. For a partial or
/// cancelled result a root counts as processed only when an item below the root exists.
fn outcomes(result: &ResultVersion) -> Vec<Outcome> {
    let count = result.roots.len();
    let complete = matches!(
        result.completeness,
        ResultCompleteness::Complete | ResultCompleteness::Missing
    );
    let mut processed = vec![complete; count];
    if !complete {
        for (item, root) in result.items.iter().zip(&result.root_indices) {
            if let Some(slot) = item.parent.and(processed.get_mut(*root as usize)) {
                *slot = true;
            }
        }
    }
    let mut failed: Vec<Option<String>> = vec![None; count];
    for gap in result.gaps.iter().filter(|gap| gap.path.is_empty()) {
        if let Some(slot) = failed.get_mut(gap.root_index as usize) {
            slot.get_or_insert_with(|| gap.reason_key.clone());
        }
    }
    failed
        .into_iter()
        .zip(processed)
        .map(|(failed, processed)| Outcome { failed, processed })
        .collect()
}
