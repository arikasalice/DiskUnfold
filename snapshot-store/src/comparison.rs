//! F08 change classification for one normalized path in two pinned results.

pub mod folder;
pub mod range;
pub mod summary;
pub mod tree;

use crate::storage_match::{self, Correspondence, Observation};
use du_core_contracts::{SizeAmount, SizeBasis, SizeValue, result::ResultItemKind};
use std::cmp::Ordering;

/// Rule identifier to record on a result produced by this decision table.
pub const CLASSIFY_RULE_ID: &str = "diff.classify";
/// Revision of the change classification rules.
pub const CLASSIFY_RULE_REVISION: u32 = 1;

/// Observed item evidence in one of the two input results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Side<'a> {
    pub item_no: u32,
    pub kind: ResultItemKind,
    /// Original size for the chosen basis, when available.
    pub size: Option<&'a SizeValue>,
}

/// Reason supplied by the comparison's range correspondence stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gap {
    DifferentStorage,
    UnknownStorage,
    OutsideScope,
    Excluded,
    AccessDenied,
    MissingObservation,
    KeyCollision,
}

/// Whether this path can be compared and absence on the later side is proven.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Comparable {
        /// None means that the later parent was completely observed.
        after_gap: Option<Gap>,
    },
    /// Includes ambiguous normalized keys. This takes precedence over item evidence.
    Incomparable(Gap),
}

/// Classification before optional move detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Added,
    Removed,
    Grown,
    Shrunk,
    Unchanged,
    SizeUnverified,
    Unverified(Gap),
    Incomparable(Gap),
}

/// One row with original evidence and an exact signed byte delta when derivable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row<'a> {
    pub change: Change,
    pub before: Option<Side<'a>>,
    pub after: Option<Side<'a>>,
    /// Unknown and lower-bound values do not establish an exact delta.
    pub delta: Option<i128>,
}

/// Rows produced for one key, in stable order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rows<'a> {
    One(Row<'a>),
    /// A known file/directory replacement produces removal followed by addition.
    TypeChanged {
        removed: Row<'a>,
        added: Row<'a>,
    },
}

/// A root comparison whose recorded storage evidence was resolved once.
///
/// This applies `compare.storage-match` before classifying individual keys.
/// Its observations must originate from the two already validated inputs.
pub struct RootComparison {
    correspondence: Correspondence,
}

impl RootComparison {
    /// Resolves the two observed storage identities without looking up current OS state.
    #[must_use]
    pub fn new(before: Observation<'_>, after: Observation<'_>) -> Self {
        Self {
            correspondence: storage_match::compare(before, after),
        }
    }

    /// Returns the pinned root-level correspondence used for each classification.
    #[must_use]
    pub fn correspondence(&self) -> Correspondence {
        self.correspondence
    }

    /// Classifies a key only after applying the root's storage boundary.
    ///
    /// Different or unverifiable storage overrides narrower coverage evidence,
    /// except that a known different-storage boundary has higher precedence.
    #[must_use]
    pub fn classify<'a>(
        &self,
        before: Option<Side<'a>>,
        after: Option<Side<'a>>,
        location: Location,
        basis: &SizeBasis,
    ) -> Option<Rows<'a>> {
        let location = match (self.correspondence, location) {
            (Correspondence::Different, _) | (_, Location::Incomparable(Gap::DifferentStorage)) => {
                Location::Incomparable(Gap::DifferentStorage)
            }
            (Correspondence::Unverifiable, _) => Location::Incomparable(Gap::UnknownStorage),
            (Correspondence::Same, location) => location,
        };
        classify(before, after, location, basis)
    }
}

/// Classifies a paired normalized key using the scope evidence supplied by the caller.
///
/// The caller is responsible for grouping by the published path comparison rule and
/// identifying collisions and incomparable ranges. No path lookup occurs here.
pub fn classify<'a>(
    before: Option<Side<'a>>,
    after: Option<Side<'a>>,
    location: Location,
    basis: &SizeBasis,
) -> Option<Rows<'a>> {
    let after_gap = match location {
        Location::Comparable { after_gap } => after_gap,
        Location::Incomparable(gap) => {
            return Some(Rows::One(Row {
                change: Change::Incomparable(gap),
                before,
                after,
                delta: None,
            }));
        }
    };
    let row = match (before, after) {
        (None, None) => return None,
        (None, Some(after)) => added(after, basis),
        (Some(before), None) => match after_gap {
            Some(gap) => Row {
                change: Change::Unverified(gap),
                before: Some(before),
                after: None,
                delta: None,
            },
            None => removed(before, basis),
        },
        (Some(before), Some(after)) if type_changed(before.kind, after.kind) => {
            return Some(Rows::TypeChanged {
                removed: removed(before, basis),
                added: added(after, basis),
            });
        }
        (Some(before), Some(after)) => compare_sizes(before, after, basis),
    };
    Some(Rows::One(row))
}

fn type_changed(before: ResultItemKind, after: ResultItemKind) -> bool {
    use ResultItemKind::{Directory, File};
    matches!((before, after), (File, Directory) | (Directory, File))
}

fn added<'a>(after: Side<'a>, basis: &SizeBasis) -> Row<'a> {
    Row {
        change: Change::Added,
        before: None,
        after: Some(after),
        delta: exact(after, basis),
    }
}

fn removed<'a>(before: Side<'a>, basis: &SizeBasis) -> Row<'a> {
    Row {
        change: Change::Removed,
        before: Some(before),
        after: None,
        delta: exact(before, basis).map(|bytes| -bytes),
    }
}

fn compare_sizes<'a>(before: Side<'a>, after: Side<'a>, basis: &SizeBasis) -> Row<'a> {
    let delta = exact(before, basis)
        .zip(exact(after, basis))
        .map(|(before, after)| after - before);
    let change = match delta.map(|delta| delta.cmp(&0)) {
        Some(Ordering::Greater) => Change::Grown,
        Some(Ordering::Less) => Change::Shrunk,
        Some(Ordering::Equal) => Change::Unchanged,
        None => Change::SizeUnverified,
    };
    Row {
        change,
        before: Some(before),
        after: Some(after),
        delta,
    }
}

fn exact(side: Side<'_>, basis: &SizeBasis) -> Option<i128> {
    let size = side.size.filter(|size| &size.basis == basis)?;
    match &size.amount {
        SizeAmount::Exact { bytes } => bytes.as_str().parse::<u64>().ok().map(i128::from),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
