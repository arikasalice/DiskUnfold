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

/// The item numbers of every set, gathered in one walk over the files.
struct Sets {
    groups: Vec<Vec<u32>>,
    union: Vec<u32>,
    overlap: Vec<u32>,
    unclassified: Vec<u32>,
    undecidable: Vec<u32>,
}

impl Classification {
    /// Calls the pinned F03 file-set operation for every category and for the scope.
    ///
    /// An empty `selected` selects every group; repeated indices count once. The selection
    /// decides the union and overlap sets; per-group totals, unclassified, undecidable and the
    /// scope total do not depend on it. Files sharing one OS file identity, such as hard links
    /// in two groups, contribute once to a set that holds them both.
    ///
    /// The sets are gathered in one walk over the files and summed one after another, so
    /// the cost is one pass plus the sums, and the gathered numbers take at most as many
    /// entries as the files times the matched groups and a few more. The totals of the
    /// default selection are calculated when the classification is built, and the last few
    /// other selections are cached.
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
        let sets = self.gather_sets(enabled, cancelled)?;
        let mut sum = |numbers: &[u32]| self.sum_set(numbers, &mut *cancelled);
        let mut groups = Vec::with_capacity(sets.groups.len());
        for numbers in &sets.groups {
            groups.push(sum(numbers)?);
        }
        Ok(Totals {
            groups,
            union: sum(&sets.union)?,
            overlap: sum(&sets.overlap)?,
            overlap_count: sets.overlap.len(),
            unclassified: sum(&sets.unclassified)?,
            undecidable: sum(&sets.undecidable)?,
            scope: sum(&self.files)?,
        })
    }

    /// Walks the files once and sorts each into every set it belongs to.
    fn gather_sets(
        &self,
        enabled: &[bool],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Sets, Error> {
        let mut sets = Sets {
            groups: vec![Vec::new(); self.rules.len()],
            union: Vec::new(),
            overlap: Vec::new(),
            unclassified: Vec::new(),
            undecidable: Vec::new(),
        };
        for (index, &item_no) in self.files.iter().enumerate() {
            if index % CHECK_INTERVAL == 0 && cancelled() {
                return Err(Error::Cancelled);
            }
            let row = self.row(index);
            let mut selected = 0usize;
            for (rule, decision) in row.iter().enumerate() {
                if *decision == Decision::Match {
                    sets.groups
                        .get_mut(rule)
                        .ok_or(Error::InvalidSelection)?
                        .push(item_no);
                    selected += usize::from(enabled.get(rule) == Some(&true));
                }
            }
            if selected > 0 {
                sets.union.push(item_no);
            }
            if selected > 1 {
                sets.overlap.push(item_no);
            }
            let tally = Tally::of(row);
            if tally.matches == 0 {
                let rest = if tally.undecided() {
                    &mut sets.undecidable
                } else {
                    &mut sets.unclassified
                };
                rest.push(item_no);
            }
        }
        Ok(sets)
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
