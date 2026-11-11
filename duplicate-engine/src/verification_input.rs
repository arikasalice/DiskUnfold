//! Approved file observations supplied to duplicate verification.

use crate::candidates::{FileIdentity, Placeholder, Size, Target};

/// State observed before a file is selected for verification.
///
/// `modified` is an adapter-defined timestamp value that must use the same resolution
/// and representation for the approval and both observations of a read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub identity: FileIdentity,
    pub size: u64,
    pub modified: u64,
    pub change_stamp: Option<u64>,
}

/// An approved target and its pre-observed state. Missing state is never opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationTarget {
    pub target: Target,
    pub snapshot: Option<Snapshot>,
}

pub(crate) fn valid_approval(
    item: &VerificationTarget,
    identity: &FileIdentity,
    size: u64,
) -> bool {
    item.target.placeholder == Placeholder::Local
        && item.target.size == Size::Exact(size)
        && item.target.identity.as_ref() == Some(identity)
        && item
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.identity == *identity && snapshot.size == size)
}

/// A verified-selection candidate and its aliases.
pub(crate) struct Candidate<'a> {
    pub(crate) approved: &'a VerificationTarget,
    pub(crate) aliases: Vec<u32>,
    pub(crate) identity: FileIdentity,
}
