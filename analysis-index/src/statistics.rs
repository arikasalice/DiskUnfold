//! Pinned, read-only statistics calculations, independent of IPC and registration.
//!
//! The caller supplies a selected set and its canonical path comparison keys.
//! This module does not define display limits or a public statistics model.
use crate::ResultVersion;
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultItemKind, ResultVersionRead},
};
use std::{cmp::Ordering, collections::HashSet};

/// A selected item and the canonical path comparison key for its pinned version.
pub struct Candidate {
    pub item: ItemRef,
    pub path_compare_key: Vec<u8>,
}

/// One item level; files and directories are never mixed in a ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    File,
    Directory,
}

/// A ranked item with the original size and its one-based position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranked {
    pub position: usize,
    pub item: ItemRef,
    pub size: SizeValue,
}

/// The selected version and basis are retained even for an empty selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranking {
    pub result_version: Id,
    pub basis: SizeBasis,
    pub ranked: Vec<Ranked>,
    /// Selected items at the requested level whose size could not be established.
    pub unknown: Vec<ItemRef>,
    /// Whether any item at this level has a known or lower-bound size before truncation.
    pub has_sizes: bool,
}

/// Invalid selections and cancellation never produce a partial ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    VersionMismatch,
    InvalidSelection,
    Cancelled,
}

struct Known<'a> {
    item: ItemRef,
    key: &'a [u8],
    size: SizeValue,
    bytes: u64,
}

/// Ranks one explicitly selected set from one already pinned immutable version.
///
/// Known and lower-bound values sort by bytes descending, then by canonical key
/// bytes ascending. Unknown values are returned separately, never as zero. The
/// caller must obtain keys using the source's path comparison rule and preserve
/// the source selection and its pin for the lifetime of the calculation.
pub fn rank_size(
    version: &ResultVersion,
    basis: SizeBasis,
    level: Level,
    limit: usize,
    candidates: &[Candidate],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Ranking, Error> {
    let mut seen = HashSet::new();
    let mut keys = HashSet::new();
    let mut known = Vec::new();
    let mut unknown = Vec::new();
    for candidate in candidates {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if candidate.item.result_version != *version.version_id() {
            return Err(Error::VersionMismatch);
        }
        if !seen.insert(candidate.item.item_no) || !keys.insert(&candidate.path_compare_key) {
            return Err(Error::InvalidSelection);
        }
        let item = version
            .items()
            .get(candidate.item.item_no as usize)
            .ok_or(Error::InvalidSelection)?;
        let matches_level = matches!(
            (level, item.kind),
            (Level::File, ResultItemKind::File) | (Level::Directory, ResultItemKind::Directory)
        );
        if !matches_level {
            continue;
        }
        match version
            .item_size(item.item_no, basis.clone())
            .and_then(|size| numeric(size, &basis).map(|bytes| (size, bytes)))
        {
            Some((size, bytes)) => known.push(Known {
                item: candidate.item.clone(),
                key: &candidate.path_compare_key,
                size: size.clone(),
                bytes,
            }),
            None => unknown.push((&candidate.path_compare_key, candidate.item.clone())),
        }
    }
    let has_sizes = !known.is_empty();
    if limit == 0 {
        known.clear();
    } else if limit < known.len() {
        known.select_nth_unstable_by(limit, compare_rank);
        known.truncate(limit);
    }
    known.sort_unstable_by(compare_rank);
    unknown.sort_by(|a, b| a.0.cmp(b.0));
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let ranked = known
        .into_iter()
        .enumerate()
        .map(|(index, row)| Ranked {
            position: index + 1,
            item: row.item,
            size: row.size,
        })
        .collect();
    Ok(Ranking {
        result_version: version.version_id().clone(),
        basis,
        ranked,
        unknown: unknown.into_iter().map(|(_, item)| item).collect(),
        has_sizes,
    })
}

fn compare_rank(left: &Known<'_>, right: &Known<'_>) -> Ordering {
    right
        .bytes
        .cmp(&left.bytes)
        .then_with(|| left.key.cmp(right.key))
}

fn numeric(size: &SizeValue, basis: &SizeBasis) -> Option<u64> {
    if &size.basis != basis || size.evidence == SizeEvidence::Unknown {
        return None;
    }
    match &size.amount {
        SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => {
            bytes.as_str().parse().ok()
        }
        SizeAmount::Unknown => None,
    }
}
