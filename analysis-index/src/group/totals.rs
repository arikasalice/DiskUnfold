//! F03 set totals for the category sets of one classification.

use du_core_contracts::result::SpaceAggregate;

use super::{Classification, Decision, Error, summary::Tally};
use crate::space::selection;

/// Non-default selections whose totals are kept per classification, oldest first.
const MAX_CACHED_TOTALS: usize = 4;
/// Files between two cancellation checks while a set is gathered.
const CHECK_INTERVAL: usize = 4096;

/// Each value is calculated independently by F03 from the corresponding file set.
/// No total is derived from another by adding or subtracting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Totals {
    /// One total per rule, in rule display order, for the files matching that rule.
    pub groups: Vec<SpaceAggregate>,
    /// Files that match at least one selected rule.
    pub union: SpaceAggregate,
    /// Files that match at least two selected rules.
    pub overlap: SpaceAggregate,
    /// Number of files in the overlap set.
    pub overlap_count: usize,
    /// Files that match no rule and have no undecided rule; independent of the selection.
    pub unclassified: SpaceAggregate,
    /// Files that match no rule and have an undecided rule; independent of the selection.
    pub undecidable: SpaceAggregate,
    /// Every file of the scope.
    pub scope: SpaceAggregate,
}

/// Totals already calculated for a selection mask. The default selection (every group)
/// is calculated when the classification is built and never evicted; other selections
/// are kept oldest first.
#[derive(Default)]
pub(super) struct TotalsCache {
    default: Option<Totals>,
    recent: Vec<(Vec<bool>, Totals)>,
}

/// Totals that do not depend on the selection.
struct Base {
    groups: Vec<SpaceAggregate>,
    unclassified: SpaceAggregate,
    undecidable: SpaceAggregate,
    scope: SpaceAggregate,
}

impl Classification {
    /// Calls the pinned F03 file-set operation for every category and for the scope.
    ///
    /// An empty `selected` selects every group; repeated indices count once. The selection
    /// decides the union and overlap sets; per-group totals, unclassified, undecidable and the
    /// scope total do not depend on it. Files sharing one OS file identity, such as hard links
    /// in two groups, contribute once to a set that holds them both.
    ///
    /// The totals of the default selection are calculated when the classification is
    /// built, one group set at a time, so gathered item numbers never exceed two entries per
    /// file. Another selection reuses the selection-independent totals and walks the files
    /// once for its union and overlap. The last few other selections are cached.
    ///
    /// # Errors
    /// Returns [`Error::InvalidSelection`] for an index without a rule,
    /// [`Error::Total`] when F03 rejects an item, [`Error::Unavailable`] for a poisoned
    /// cache and [`Error::Cancelled`].
    pub fn totals(
        &self,
        selected: &[usize],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Totals, Error> {
        let mut enabled = vec![selected.is_empty(); self.rules.len()];
        for &index in selected {
            *enabled.get_mut(index).ok_or(Error::InvalidSelection)? = true;
        }
        if let Some(found) = self.cached_totals(&enabled)? {
            return Ok(found);
        }
        let totals = self.compute_totals(&enabled, &mut cancelled)?;
        self.cache_totals(enabled, &totals)?;
        Ok(totals)
    }

    /// Calculates the totals of the default selection for the first read of the
    /// classification, so `group.totals` does not walk the scope.
    pub(super) fn prime_totals(&self, mut cancelled: impl FnMut() -> bool) -> Result<(), Error> {
        let enabled = vec![true; self.rules.len()];
        let totals = self.compute_totals(&enabled, &mut cancelled)?;
        self.cache_totals(enabled, &totals)
    }

    fn compute_totals(
        &self,
        enabled: &[bool],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Totals, Error> {
        let base = match self.default_totals()? {
            Some(totals) => Base {
                groups: totals.groups,
                unclassified: totals.unclassified,
                undecidable: totals.undecidable,
                scope: totals.scope,
            },
            None => self.base_totals(cancelled)?,
        };
        let mut union = Vec::new();
        let mut overlap = Vec::new();
        self.walk(cancelled, |row, item_no| {
            let selected = row
                .iter()
                .zip(enabled)
                .filter(|(decision, on)| **on && **decision == Decision::Match)
                .count();
            if selected > 0 {
                union.push(item_no);
            }
            if selected > 1 {
                overlap.push(item_no);
            }
        })?;
        Ok(Totals {
            groups: base.groups,
            union: self.sum_set(&union, cancelled)?,
            overlap_count: overlap.len(),
            overlap: self.sum_set(&overlap, cancelled)?,
            unclassified: base.unclassified,
            undecidable: base.undecidable,
            scope: base.scope,
        })
    }

    /// Sums each group set in its own walk, then the unclassified and undecidable sets in
    /// one walk, so at most one group set or two disjoint sets are held at a time.
    fn base_totals(&self, cancelled: &mut impl FnMut() -> bool) -> Result<Base, Error> {
        let mut groups = Vec::with_capacity(self.rules.len());
        for rule in 0..self.rules.len() {
            let mut members = Vec::new();
            self.walk(cancelled, |row, item_no| {
                if row.get(rule) == Some(&Decision::Match) {
                    members.push(item_no);
                }
            })?;
            groups.push(self.sum_set(&members, cancelled)?);
        }
        let mut unclassified = Vec::new();
        let mut undecidable = Vec::new();
        self.walk(cancelled, |row, item_no| {
            let tally = Tally::of(row);
            if tally.matches == 0 {
                if tally.undecided() {
                    undecidable.push(item_no);
                } else {
                    unclassified.push(item_no);
                }
            }
        })?;
        Ok(Base {
            groups,
            unclassified: self.sum_set(&unclassified, cancelled)?,
            undecidable: self.sum_set(&undecidable, cancelled)?,
            scope: self.sum_set(&self.files, cancelled)?,
        })
    }

    /// Visits every file's decisions in path order, checking for cancellation regularly.
    fn walk(
        &self,
        cancelled: &mut impl FnMut() -> bool,
        mut visit: impl FnMut(&[Decision], u32),
    ) -> Result<(), Error> {
        for (index, &item_no) in self.files.iter().enumerate() {
            if index % CHECK_INTERVAL == 0 && cancelled() {
                return Err(Error::Cancelled);
            }
            visit(self.row(index), item_no);
        }
        Ok(())
    }

    fn default_totals(&self) -> Result<Option<Totals>, Error> {
        let cache = self.totals.lock().map_err(|_| Error::Unavailable)?;
        Ok(cache.default.clone())
    }

    fn sum_set(
        &self,
        numbers: &[u32],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<SpaceAggregate, Error> {
        selection::sum_item_numbers(&self.version, numbers, self.coverage_complete, cancelled)
            .map_err(|error| match error {
                selection::Error::Cancelled => Error::Cancelled,
                error => Error::Total(error),
            })
    }

    fn cached_totals(&self, enabled: &[bool]) -> Result<Option<Totals>, Error> {
        let cache = self.totals.lock().map_err(|_| Error::Unavailable)?;
        if enabled.iter().all(|on| *on)
            && let Some(totals) = &cache.default
        {
            return Ok(Some(totals.clone()));
        }
        Ok(cache
            .recent
            .iter()
            .find(|(mask, _)| mask.as_slice() == enabled)
            .map(|(_, totals)| totals.clone()))
    }

    fn cache_totals(&self, enabled: Vec<bool>, totals: &Totals) -> Result<(), Error> {
        let mut cache = self.totals.lock().map_err(|_| Error::Unavailable)?;
        if enabled.iter().all(|on| *on) {
            cache.default = Some(totals.clone());
            return Ok(());
        }
        cache.recent.retain(|(mask, _)| mask != &enabled);
        if cache.recent.len() >= MAX_CACHED_TOTALS {
            cache.recent.remove(0);
        }
        cache.recent.push((enabled, totals.clone()));
        Ok(())
    }
}
