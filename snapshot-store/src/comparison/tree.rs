//! Deterministic F08 tree assembly over already verified snapshot item records.
use super::{Change, Gap, Location, RootComparison, folder};
use crate::records::item::{Item, Kind};
use du_core_contracts::SizeBasis;
use std::collections::BTreeMap;

/// An immutable index for one verified analysis root and its component keys.
pub struct Tree<'a> {
    children: BTreeMap<u32, Vec<folder::Child<'a>>>,
}

/// Invalid inputs and cancellation never expose a partial tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidTree,
    Cancelled,
}

impl<'a> Tree<'a> {
    /// Indexes a verified record sequence without deriving comparison keys.
    /// Parent zero denotes the virtual root. Other parents must exist, be folders,
    /// and have lower positive numbers, preventing cycles and unreachable nodes.
    ///
    /// # Errors
    /// Returns `InvalidTree` for duplicate IDs, invalid ancestry, or a non-folder parent.
    /// Returns `Cancelled` if the caller requests cancellation during admission.
    pub fn new(
        items: &[folder::Child<'a>],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, Error> {
        let mut by_id = BTreeMap::new();
        for child in items {
            check(&mut cancelled)?;
            if child.item.number == 0
                || child.item.parent >= child.item.number
                || by_id.insert(child.item.number, child.item.kind).is_some()
            {
                return Err(Error::InvalidTree);
            }
        }
        let mut children: BTreeMap<u32, Vec<folder::Child<'a>>> = BTreeMap::new();
        for child in items {
            check(&mut cancelled)?;
            if child.item.parent != 0 && by_id.get(&child.item.parent) != Some(&Kind::Folder) {
                return Err(Error::InvalidTree);
            }
            children.entry(child.item.parent).or_default().push(*child);
        }
        check(&mut cancelled)?;
        Ok(Self { children })
    }
    fn children(&self, parent: Option<u32>) -> &[folder::Child<'a>] {
        parent
            .and_then(|id| self.children.get(&id))
            .map_or(&[], Vec::as_slice)
    }
}

/// A pre-order change row and its normalized component path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row<'a> {
    pub path: Vec<Vec<u8>>,
    pub before: Option<&'a Item<'a>>,
    pub after: Option<&'a Item<'a>>,
    pub change: Change,
    pub delta: Option<i128>,
}

struct Visit {
    parents: (Option<u32>, Option<u32>),
    path: Vec<Vec<u8>>,
    inherited_gap: Option<Gap>,
}
enum Task<'a> {
    Visit(Visit),
    Emit(Row<'a>),
}

/// Assembles direct-child fragments depth first without reading or modifying sources.
///
/// The source provider must partition both inputs by analysis root and supply
/// the matching storage decision, verified records, normalized keys and complete
/// range decisions for each full path, including inherited scope restrictions.
/// Incomparable folders are not descended. A missing later-side parent preserves
/// its unverified cause for descendants. A one-sided folder carries its absence
/// through the subtree, subject to each path's stronger range decision.
/// Sorting is local to a folder; worker parallelism and bounded streaming are not
/// implemented by this sequential assembler.
///
/// # Errors
/// Returns `Cancelled` without partial output if cancellation is observed.
pub fn compare<'a>(
    root: &RootComparison,
    before: &Tree<'a>,
    after: &Tree<'a>,
    basis: &SizeBasis,
    mut location: impl FnMut(&[Vec<u8>]) -> Location,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<Row<'a>>, Error> {
    let mut tasks = vec![Task::Visit(Visit {
        parents: (Some(0), Some(0)),
        path: Vec::new(),
        inherited_gap: None,
    })];
    let mut output = Vec::new();
    while let Some(task) = tasks.pop() {
        check(&mut cancelled)?;
        match task {
            Task::Emit(row) => output.push(row),
            Task::Visit(mut visit) => {
                let rows = folder::merge(
                    root,
                    (visit.parents.0.unwrap_or(0), visit.parents.1.unwrap_or(0)),
                    before.children(visit.parents.0),
                    after.children(visit.parents.1),
                    basis,
                    |key| {
                        visit.path.push(key.to_vec());
                        let decision = location(&visit.path);
                        visit.path.pop();
                        inherited_location(decision, visit.inherited_gap)
                    },
                    &mut cancelled,
                )
                .map_err(|error| match error {
                    folder::Error::Cancelled => Error::Cancelled,
                    folder::Error::WrongParent => Error::InvalidTree,
                })?;
                for row in rows.into_iter().rev() {
                    check(&mut cancelled)?;
                    let mut path = visit.path.clone();
                    path.push(row.key.to_vec());
                    let inherited_gap = match row.change {
                        Change::Unverified(gap) => Some(gap),
                        _ => visit.inherited_gap,
                    };
                    let descend = !matches!(row.change, Change::Incomparable(_))
                        && (row.before.is_some_and(|item| item.kind == Kind::Folder)
                            || row.after.is_some_and(|item| item.kind == Kind::Folder));
                    if descend {
                        tasks.push(Task::Visit(Visit {
                            parents: (
                                row.before
                                    .filter(|item| item.kind == Kind::Folder)
                                    .map(|item| item.number),
                                row.after
                                    .filter(|item| item.kind == Kind::Folder)
                                    .map(|item| item.number),
                            ),
                            path: path.clone(),
                            inherited_gap,
                        }));
                    }
                    tasks.push(Task::Emit(Row {
                        path,
                        before: row.before,
                        after: row.after,
                        change: row.change,
                        delta: row.delta,
                    }));
                }
            }
        }
    }
    check(&mut cancelled)?;
    Ok(output)
}
fn check(cancelled: &mut impl FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

fn inherited_location(location: Location, inherited: Option<Gap>) -> Location {
    let Some(inherited) = inherited else {
        return location;
    };
    match location {
        Location::Comparable { after_gap } => Location::Comparable {
            after_gap: Some(after_gap.map_or(inherited, |gap| stronger(inherited, gap))),
        },
        Location::Incomparable(gap) => Location::Incomparable(stronger(inherited, gap)),
    }
}

pub(super) fn stronger(a: Gap, b: Gap) -> Gap {
    fn priority(gap: Gap) -> u8 {
        match gap {
            Gap::DifferentStorage => 6,
            Gap::UnknownStorage => 5,
            Gap::OutsideScope => 4,
            Gap::Excluded => 3,
            Gap::AccessDenied => 2,
            Gap::MissingObservation => 1,
            Gap::KeyCollision => 0,
        }
    }
    if priority(a) >= priority(b) { a } else { b }
}

#[cfg(test)]
mod tests;
