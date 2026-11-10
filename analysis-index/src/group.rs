//! File membership and category totals from one immutable analysis result.

use std::sync::Arc;

use du_core_contracts::{
    ItemRef,
    result::{ResultItemKind, SpaceAggregate},
};

use crate::{
    QueryError, ResultVersion,
    query::{self, Condition, Record, Registry, Truth, plan},
    space::selection,
};

/// A rule's decision for one file, in display order of the supplied rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Match,
    NoMatch,
    Unknown,
    RuleUnavailable,
}

impl Decision {
    /// Returns the established reason for a rule whose strategy is unavailable.
    pub fn reason_key(self) -> Option<&'static str> {
        match self {
            Self::RuleUnavailable => Some("group.rule_unevaluable"),
            _ => None,
        }
    }
}

/// One confirmed file and its rule decisions.
#[derive(Debug, PartialEq, Eq)]
pub struct Member {
    pub item: ItemRef,
    pub decisions: Vec<Decision>,
}

/// A classification read holding the exact source used for its decisions and totals.
pub struct Classification {
    version: Arc<ResultVersion>,
    root: ItemRef,
    rows: Vec<Member>,
    rule_count: usize,
    coverage_complete: bool,
}

/// Category sets in observed path order. Unclassified and undecidable use all rules.
#[derive(Debug, PartialEq, Eq)]
pub struct Categories {
    pub groups: Vec<Vec<ItemRef>>,
    pub union: Vec<ItemRef>,
    pub overlap: Vec<ItemRef>,
    pub unclassified: Vec<ItemRef>,
    pub undecidable: Vec<ItemRef>,
}

/// Each value is calculated independently by F03 from the corresponding item set.
pub struct Totals {
    pub groups: Vec<SpaceAggregate>,
    pub union: SpaceAggregate,
    pub overlap: SpaceAggregate,
    pub unclassified: SpaceAggregate,
    pub undecidable: SpaceAggregate,
    pub scope: SpaceAggregate,
}

/// Admission or evaluation failure; a cancelled operation returns no partial value.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Source(QueryError),
    InvalidRoot,
    InvalidRule(query::Error),
    InvalidSelection,
    Total(query::SpaceTotalError),
    Cancelled,
}

/// Resolves rules once and evaluates confirmed files below the pinned scope root.
/// Sibling traversal uses the result's stored path order. Unknown decisions are
/// retained per rule and a missing strategy is recorded for each affected file.
pub fn classify(
    version: Arc<ResultVersion>,
    root: &ItemRef,
    registry: &Registry,
    rules: &[Condition],
    mut cancelled: impl FnMut() -> bool,
) -> Result<Classification, Error> {
    let folder = version.detail(root).map_err(Error::Source)?.item;
    if folder.kind != ResultItemKind::Directory {
        return Err(Error::InvalidRoot);
    }
    let plans = prepare_rules(registry, rules, &mut cancelled)?;
    let mut rows = Vec::new();
    let mut coverage_complete = true;
    let mut branches = vec![version.children(folder.item_no).iter()];
    while let Some(branch) = branches.last_mut() {
        check(&mut cancelled)?;
        let Some(item) = branch.next() else {
            branches.pop();
            continue;
        };
        if item.kind == ResultItemKind::File {
            let path = query::navigation_path(&version, item, None);
            let record = Record {
                item,
                path: &path,
                version: &version,
            };
            rows.push(Member {
                item: ItemRef {
                    result_version: root.result_version.clone(),
                    item_no: item.item_no,
                },
                decisions: evaluate(&plans, &record, &mut cancelled)?,
            });
        }
        coverage_complete &= matches!(item.kind, ResultItemKind::File | ResultItemKind::Directory);
        let children = version.children(item.item_no);
        if !children.is_empty() {
            branches.push(children.iter());
        }
    }
    check(&mut cancelled)?;
    Ok(Classification {
        version,
        root: root.clone(),
        rows,
        rule_count: rules.len(),
        coverage_complete,
    })
}

fn prepare_rules<'a>(
    registry: &Registry,
    rules: &'a [Condition],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<Option<plan::Plan<'a>>>, Error> {
    rules
        .iter()
        .map(|rule| {
            check(cancelled)?;
            query::validate_condition(rule).map_err(Error::InvalidRule)?;
            match plan::prepare(registry, rule, 0) {
                Ok(plan) => Ok(Some(plan)),
                Err(query::Error::UnknownPredicate) => Ok(None),
                Err(error) => Err(Error::InvalidRule(error)),
            }
        })
        .collect()
}

fn evaluate(
    plans: &[Option<plan::Plan<'_>>],
    record: &Record<'_>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<Decision>, Error> {
    plans
        .iter()
        .map(|plan| {
            check(cancelled)?;
            Ok(match plan.as_ref().map(|plan| plan.evaluate(record)) {
                Some(Truth::Match) => Decision::Match,
                Some(Truth::NoMatch) => Decision::NoMatch,
                Some(Truth::Unknown) => Decision::Unknown,
                None => Decision::RuleUnavailable,
            })
        })
        .collect()
}

impl Classification {
    /// Returns the scope reference in the same version as every member row.
    pub fn root(&self) -> &ItemRef {
        &self.root
    }

    /// Returns file membership from the pinned classification read.
    pub fn rows(&self) -> &[Member] {
        &self.rows
    }

    /// Computes sets for all individual groups and the requested union and overlap.
    /// An empty selection selects every group. Repeated indices count once.
    pub fn categories(
        &self,
        selected: &[usize],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Categories, Error> {
        let mut enabled = vec![selected.is_empty(); self.rule_count];
        for &index in selected {
            check(&mut cancelled)?;
            *enabled.get_mut(index).ok_or(Error::InvalidSelection)? = true;
        }
        let mut categories = Categories {
            groups: vec![Vec::new(); self.rule_count],
            union: Vec::new(),
            overlap: Vec::new(),
            unclassified: Vec::new(),
            undecidable: Vec::new(),
        };
        for row in &self.rows {
            check(&mut cancelled)?;
            categories.add(row, &enabled);
        }
        check(&mut cancelled)?;
        Ok(categories)
    }

    /// Calls the pinned F03 file-set operation for every category and the scope.
    pub fn totals(
        &self,
        selected: &[usize],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Totals, Error> {
        let sets = self.categories(selected, &mut cancelled)?;
        let mut scope = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            check(&mut cancelled)?;
            scope.push(row.item.clone());
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
            unclassified: sum(&sets.unclassified)?,
            undecidable: sum(&sets.undecidable)?,
            scope: sum(&scope)?,
        })
    }
}

impl Categories {
    fn add(&mut self, row: &Member, enabled: &[bool]) {
        let mut any_match = false;
        let mut unknown = false;
        let mut selected_matches = 0;
        for (index, decision) in row.decisions.iter().enumerate() {
            match decision {
                Decision::Match => {
                    self.groups[index].push(row.item.clone());
                    any_match = true;
                    selected_matches += usize::from(enabled[index]);
                }
                Decision::Unknown | Decision::RuleUnavailable => unknown = true,
                Decision::NoMatch => {}
            }
        }
        if selected_matches > 0 {
            self.union.push(row.item.clone());
        }
        if selected_matches > 1 {
            self.overlap.push(row.item.clone());
        }
        match (any_match, unknown) {
            (false, true) => self.undecidable.push(row.item.clone()),
            (false, false) => self.unclassified.push(row.item.clone()),
            (true, _) => {}
        }
    }
}

fn check(cancelled: &mut impl FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
