//! Component-prefix range evidence for the F08 comparison assembler.
use super::{Gap, Location, RootComparison, tree, tree::stronger};
use du_core_contracts::SizeBasis;
use std::collections::BTreeMap;

/// A side on which a range is known to be unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Earlier,
    Later,
}

/// Reasons a whole range cannot be compared on either side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeReason {
    OutsideScope,
    Excluded,
    AccessDenied,
    MissingObservation,
}

impl RangeReason {
    fn gap(self) -> Gap {
        match self {
            Self::OutsideScope => Gap::OutsideScope,
            Self::Excluded => Gap::Excluded,
            Self::AccessDenied => Gap::AccessDenied,
            Self::MissingObservation => Gap::MissingObservation,
        }
    }
}

/// Evidence that the later folder was not completely observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Incomplete {
    AccessDenied,
    MissingObservation,
}

impl Incomplete {
    fn gap(self) -> Gap {
        match self {
            Self::AccessDenied => Gap::AccessDenied,
            Self::MissingObservation => Gap::MissingObservation,
        }
    }
}

/// An inclusive unavailable range or a later folder's incomplete observation.
pub enum Evidence<'a> {
    Range {
        path: &'a [Vec<u8>],
        side: Side,
        reason: RangeReason,
    },
    LaterFolder {
        path: &'a [Vec<u8>],
        reason: Incomplete,
    },
}

#[derive(Default)]
struct Node {
    children: BTreeMap<Vec<u8>, usize>,
    earlier_range: Option<Gap>,
    later_range: Option<Gap>,
    later_folder: Option<Gap>,
}

/// Immutable range index for one pair of already matched analysis roots.
///
/// Paths are normalized component keys, not byte-string prefixes. Evidence is
/// supplied from the pinned inputs; this index neither reads nor infers it.
pub struct Ranges {
    // Nodes own child indices so even a very deep input drops iteratively.
    nodes: Vec<Node>,
}

impl Ranges {
    /// Builds the index without exposing it if cancellation is requested.
    ///
    /// Whole-range restrictions apply to the named path and descendants. A
    /// later-folder observation applies only to its direct children: the folder
    /// itself is classified using its own parent's observation.
    ///
    /// # Errors
    /// Returns `Cancelled` if cancellation is observed during construction.
    pub fn new(
        evidence: &[Evidence<'_>],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, tree::Error> {
        let mut nodes = vec![Node::default()];
        for entry in evidence {
            check(&mut cancelled)?;
            let (path, gap) = match entry {
                Evidence::Range { path, reason, .. } => (*path, reason.gap()),
                Evidence::LaterFolder { path, reason } => (*path, reason.gap()),
            };
            let mut index = 0;
            for component in path {
                check(&mut cancelled)?;
                let next = nodes.len();
                index = *nodes[index]
                    .children
                    .entry(component.clone())
                    .or_insert(next);
                if index == next {
                    nodes.push(Node::default());
                }
            }
            let node = &mut nodes[index];
            let target = match entry {
                Evidence::Range {
                    side: Side::Earlier,
                    ..
                } => &mut node.earlier_range,
                Evidence::Range {
                    side: Side::Later, ..
                } => &mut node.later_range,
                Evidence::LaterFolder { .. } => &mut node.later_folder,
            };
            *target = Some(target.map_or(gap, |old| stronger(old, gap)));
        }
        check(&mut cancelled)?;
        Ok(Self { nodes })
    }

    /// Returns the applicable inclusive range and later-parent decision.
    #[must_use]
    pub fn location(&self, path: &[Vec<u8>]) -> Location {
        let mut node = &self.nodes[0];
        let mut range = combine(node.earlier_range, node.later_range);
        let mut later = None;
        for (index, component) in path.iter().enumerate() {
            later = node.later_folder;
            let Some(next) = node.children.get(component) else {
                if index + 1 < path.len() {
                    later = None;
                }
                break;
            };
            node = &self.nodes[*next];
            range = combine(range, combine(node.earlier_range, node.later_range));
        }
        match range {
            Some(gap) => Location::Incomparable(later.map_or(gap, |other| stronger(gap, other))),
            None => Location::Comparable { after_gap: later },
        }
    }
}

fn combine(a: Option<Gap>, b: Option<Gap>) -> Option<Gap> {
    match (a, b) {
        (Some(a), Some(b)) => Some(stronger(a, b)),
        (Some(gap), None) | (None, Some(gap)) => Some(gap),
        (None, None) => None,
    }
}

fn check(cancelled: &mut impl FnMut() -> bool) -> Result<(), tree::Error> {
    if cancelled() {
        Err(tree::Error::Cancelled)
    } else {
        Ok(())
    }
}

/// Compares the pinned trees using indexed range evidence and root storage identity.
///
/// # Errors
/// Returns `Cancelled` without partial output when cancellation is observed.
pub fn compare<'a>(
    root: &RootComparison,
    before: &tree::Tree<'a>,
    after: &tree::Tree<'a>,
    basis: &SizeBasis,
    ranges: &Ranges,
    cancelled: impl FnMut() -> bool,
) -> Result<Vec<tree::Row<'a>>, tree::Error> {
    tree::compare(
        root,
        before,
        after,
        basis,
        |path| ranges.location(path),
        cancelled,
    )
}

#[cfg(test)]
mod tests;
