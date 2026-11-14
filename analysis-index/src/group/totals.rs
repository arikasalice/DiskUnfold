//! F03 set totals for the category sets of one classification.

use du_core_contracts::{
    ItemRef,
    result::{ResultVersionRead, SpaceAggregate},
};

use super::{Classification, Decision, Error};
use crate::space::selection;

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

/// The file sets that are summed, each in observed path order.
struct Sets {
    groups: Vec<Vec<ItemRef>>,
    union: Vec<ItemRef>,
    overlap: Vec<ItemRef>,
    unclassified: Vec<ItemRef>,
    undecidable: Vec<ItemRef>,
    scope: Vec<ItemRef>,
}

impl Sets {
    fn with_groups(count: usize) -> Self {
        Self {
            groups: vec![Vec::new(); count],
            union: Vec::new(),
            overlap: Vec::new(),
            unclassified: Vec::new(),
            undecidable: Vec::new(),
            scope: Vec::new(),
        }
    }

    fn add(&mut self, item: ItemRef, row: &[Decision], enabled: &[bool]) {
        let mut selected_matches = 0usize;
        let mut matches = 0usize;
        let mut undecided = false;
        for (index, decision) in row.iter().enumerate() {
            match decision {
                Decision::Match => {
                    matches += 1;
                    selected_matches += usize::from(enabled.get(index) == Some(&true));
                    if let Some(group) = self.groups.get_mut(index) {
                        group.push(item.clone());
                    }
                }
                Decision::Unknown | Decision::RuleUnavailable => undecided = true,
                Decision::NoMatch => {}
            }
        }
        if selected_matches > 0 {
            self.union.push(item.clone());
        }
        if selected_matches > 1 {
            self.overlap.push(item.clone());
        }
        if matches == 0 {
            if undecided {
                self.undecidable.push(item.clone());
            } else {
                self.unclassified.push(item.clone());
            }
        }
        self.scope.push(item);
    }
}

impl Classification {
    /// Calls the pinned F03 file-set operation for every category and for the scope.
    ///
    /// An empty `selected` selects every group; repeated indices count once. The selection
    /// decides the union and overlap sets; per-group totals, unclassified, undecidable and the
    /// scope total do not depend on it. Files sharing one OS file identity, such as hard links
    /// in two groups, contribute once to a set that holds them both.
    ///
    /// # Errors
    /// Returns [`Error::InvalidSelection`] for an index without a rule,
    /// [`Error::Total`] when F03 rejects a reference, and [`Error::Cancelled`].
    pub fn totals(
        &self,
        selected: &[usize],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Totals, Error> {
        let mut enabled = vec![selected.is_empty(); self.rules.len()];
        for &index in selected {
            *enabled.get_mut(index).ok_or(Error::InvalidSelection)? = true;
        }
        let mut sets = Sets::with_groups(self.rules.len());
        for (index, &item_no) in self.files.iter().enumerate() {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let item = ItemRef {
                result_version: self.version.version_id().clone(),
                item_no,
            };
            sets.add(item, self.row(index), &enabled);
        }
        let mut sum = |items: &[ItemRef]| {
            selection::sum(&self.version, items, self.coverage_complete, &mut cancelled).map_err(
                |error| match error {
                    selection::Error::Cancelled => Error::Cancelled,
                    error => Error::Total(error),
                },
            )
        };
        let groups = sets
            .groups
            .iter()
            .map(|items| sum(items))
            .collect::<Result<_, _>>()?;
        Ok(Totals {
            groups,
            union: sum(&sets.union)?,
            overlap: sum(&sets.overlap)?,
            overlap_count: sets.overlap.len(),
            unclassified: sum(&sets.unclassified)?,
            undecidable: sum(&sets.undecidable)?,
            scope: sum(&sets.scope)?,
        })
    }
}
