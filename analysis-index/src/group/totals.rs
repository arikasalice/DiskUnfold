//! F03 set totals for the category sets of one classification.

use du_core_contracts::result::SpaceAggregate;

use super::{Classification, Decision, Error, summary::Tally};
use crate::space::selection;

/// Selections whose totals are kept per classification, oldest first.
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

/// Totals already calculated for a selection mask, oldest first.
#[derive(Default)]
pub(super) struct TotalsCache(Vec<(Vec<bool>, Totals)>);

impl Classification {
    /// Calls the pinned F03 file-set operation for every category and for the scope.
    ///
    /// An empty `selected` selects every group; repeated indices count once. The selection
    /// decides the union and overlap sets; per-group totals, unclassified, undecidable and the
    /// scope total do not depend on it. Files sharing one OS file identity, such as hard links
    /// in two groups, contribute once to a set that holds them both.
    ///
    /// Each set is gathered as item numbers and summed before the next one is gathered, so
    /// memory stays proportional to one set. The last few selections are cached.
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
        let mut groups = Vec::with_capacity(self.rules.len());
        for rule in 0..self.rules.len() {
            let filter = |row: &[Decision]| row.get(rule) == Some(&Decision::Match);
            groups.push(self.set_total(filter, &mut cancelled)?.0);
        }
        let selected_matches = |row: &[Decision]| {
            row.iter()
                .zip(&enabled)
                .filter(|(decision, on)| **on && **decision == Decision::Match)
                .count()
        };
        let (overlap, overlap_count) =
            self.set_total(|row| selected_matches(row) > 1, &mut cancelled)?;
        let totals = Totals {
            groups,
            union: self
                .set_total(|row| selected_matches(row) > 0, &mut cancelled)?
                .0,
            overlap,
            overlap_count,
            unclassified: self
                .set_total(
                    |row| {
                        let tally = Tally::of(row);
                        tally.matches == 0 && !tally.undecided()
                    },
                    &mut cancelled,
                )?
                .0,
            undecidable: self
                .set_total(
                    |row| {
                        let tally = Tally::of(row);
                        tally.matches == 0 && tally.undecided()
                    },
                    &mut cancelled,
                )?
                .0,
            scope: self.set_total(|_| true, &mut cancelled)?.0,
        };
        self.cache_totals(enabled, &totals)?;
        Ok(totals)
    }

    /// Gathers the item numbers of the files whose decisions pass `filter` and sums them.
    fn set_total(
        &self,
        filter: impl Fn(&[Decision]) -> bool,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<(SpaceAggregate, usize), Error> {
        let mut numbers = Vec::new();
        for (index, &item_no) in self.files.iter().enumerate() {
            if index % CHECK_INTERVAL == 0 && cancelled() {
                return Err(Error::Cancelled);
            }
            if filter(self.row(index)) {
                numbers.push(item_no);
            }
        }
        let total = selection::sum_item_numbers(
            &self.version,
            &numbers,
            self.coverage_complete,
            &mut *cancelled,
        )
        .map_err(|error| match error {
            selection::Error::Cancelled => Error::Cancelled,
            error => Error::Total(error),
        })?;
        Ok((total, numbers.len()))
    }

    fn cached_totals(&self, enabled: &[bool]) -> Result<Option<Totals>, Error> {
        let cache = self.totals.lock().map_err(|_| Error::Unavailable)?;
        Ok(cache
            .0
            .iter()
            .find(|(mask, _)| mask.as_slice() == enabled)
            .map(|(_, totals)| totals.clone()))
    }

    fn cache_totals(&self, enabled: Vec<bool>, totals: &Totals) -> Result<(), Error> {
        let mut cache = self.totals.lock().map_err(|_| Error::Unavailable)?;
        cache.0.retain(|(mask, _)| mask != &enabled);
        if cache.0.len() >= MAX_CACHED_TOTALS {
            cache.0.remove(0);
        }
        cache.0.push((enabled, totals.clone()));
        Ok(())
    }
}
