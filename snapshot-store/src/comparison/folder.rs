//! One F08 folder-pair merge over validated v1 item observations.
use super::{Change, Gap, Location, RootComparison, Rows, Side};
use crate::records::item::{Item, Kind, Size};
use du_core_contracts::{SizeAmount, SizeBasis, SizeEvidence, SizeValue, result::ResultItemKind};

/// A validated record and its caller-supplied normalized component comparison key.
#[derive(Clone, Copy)]
pub struct Child<'a> {
    pub item: &'a Item<'a>,
    pub key: &'a [u8],
}

/// One classified row retains original records rather than inferred metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row<'a> {
    pub key: &'a [u8],
    pub before: Option<&'a Item<'a>>,
    pub after: Option<&'a Item<'a>>,
    pub change: Change,
    pub delta: Option<i128>,
}

/// A cancelled or structurally invalid folder-pair merge has no partial result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Cancelled,
    WrongParent,
}

/// Merges direct children of a pinned folder pair in key and raw-name order.
///
/// Keys and range decisions must come from the published comparison and scope
/// owners. Records must already be verified; this function never infers keys,
/// traverses descendants, reads storage, or changes either source. A collision
/// is retained as separately unpaired incomparable rows. Cancellation is checked
/// before and after each sort and for each key; a sort itself is not interruptible.
/// The caller must not schedule descendants of an incomparable folder row.
///
/// # Errors
/// Returns `WrongParent` for a child outside its supplied folder and `Cancelled`
/// without returning partial rows when a checkpoint requests cancellation.
pub fn merge<'a>(
    root: &RootComparison,
    parents: (u32, u32),
    before: &[Child<'a>],
    after: &[Child<'a>],
    basis: &SizeBasis,
    mut location: impl FnMut(&[u8]) -> Location,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<Row<'a>>, Error> {
    check(&mut cancelled)?;
    if before.iter().any(|child| child.item.parent != parents.0)
        || after.iter().any(|child| child.item.parent != parents.1)
    {
        return Err(Error::WrongParent);
    }
    let mut left = before.to_vec();
    let mut right = after.to_vec();
    sort(&mut left, &mut cancelled)?;
    sort(&mut right, &mut cancelled)?;
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < left.len() || j < right.len() {
        check(&mut cancelled)?;
        let key = match (left.get(i), right.get(j)) {
            (Some(a), Some(b)) => a.key.min(b.key),
            (Some(a), None) => a.key,
            (None, Some(b)) => b.key,
            (None, None) => break,
        };
        let left_end = group_end(&left, i, key);
        let right_end = group_end(&right, j, key);
        let range = location(key);
        if left_end - i > 1 || right_end - j > 1 {
            let range = match range {
                Location::Incomparable(_) => range,
                _ => Location::Incomparable(Gap::KeyCollision),
            };
            for child in &left[i..left_end] {
                check(&mut cancelled)?;
                append(root, Some(*child), None, range, basis, &mut out);
            }
            for child in &right[j..right_end] {
                check(&mut cancelled)?;
                append(root, None, Some(*child), range, basis, &mut out);
            }
        } else {
            append(
                root,
                left.get(i).filter(|c| c.key == key).copied(),
                right.get(j).filter(|c| c.key == key).copied(),
                range,
                basis,
                &mut out,
            );
        }
        i = left_end;
        j = right_end;
    }
    check(&mut cancelled)?;
    Ok(out)
}
fn check(cancelled: &mut impl FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
fn sort(children: &mut [Child<'_>], cancelled: &mut impl FnMut() -> bool) -> Result<(), Error> {
    check(cancelled)?;
    children.sort_by(|a, b| {
        a.key
            .cmp(b.key)
            .then_with(|| a.item.name.cmp(b.item.name))
            .then_with(|| a.item.number.cmp(&b.item.number))
    });
    check(cancelled)
}
fn group_end(children: &[Child<'_>], start: usize, key: &[u8]) -> usize {
    let mut end = start;
    while end < children.len() && children[end].key == key {
        end += 1;
    }
    end
}
fn size(item: &Item<'_>, basis: &SizeBasis) -> SizeValue {
    let index = match basis {
        SizeBasis::Logical => 0,
        SizeBasis::Allocated => 1,
        SizeBasis::UniquePhysical => 2,
    };
    let amount = match item.sizes[index] {
        Size::Exact(bytes) => SizeAmount::Exact {
            bytes: bytes.into(),
        },
        Size::LowerBound(bytes) => SizeAmount::LowerBound {
            bytes: bytes.into(),
        },
        Size::Unknown => SizeAmount::Unknown,
    };
    SizeValue {
        basis: basis.clone(),
        evidence: SizeEvidence::Unknown,
        amount,
    }
}
fn side<'a>(child: Child<'_>, size: &'a SizeValue) -> Side<'a> {
    let kind = match child.item.kind {
        Kind::File => ResultItemKind::File,
        Kind::Folder => ResultItemKind::Directory,
        Kind::Link | Kind::Other => ResultItemKind::Unknown,
    };
    Side {
        item_no: child.item.number,
        kind,
        size: Some(size),
    }
}
fn append<'a>(
    root: &RootComparison,
    before: Option<Child<'a>>,
    after: Option<Child<'a>>,
    location: Location,
    basis: &SizeBasis,
    out: &mut Vec<Row<'a>>,
) {
    let before_size = before.map(|child| size(child.item, basis));
    let after_size = after.map(|child| size(child.item, basis));
    let result = root.classify(
        before.zip(before_size.as_ref()).map(|(c, s)| side(c, s)),
        after.zip(after_size.as_ref()).map(|(c, s)| side(c, s)),
        location,
        basis,
    );
    let key = before.or(after).map(|child| child.key).unwrap_or_default();
    let make = |row: super::Row<'_>| Row {
        key,
        before: before
            .filter(|_| row.before.is_some())
            .map(|child| child.item),
        after: after
            .filter(|_| row.after.is_some())
            .map(|child| child.item),
        change: row.change,
        delta: row.delta,
    };
    match result {
        Some(Rows::One(row)) => out.push(make(row)),
        Some(Rows::TypeChanged { removed, added }) => {
            out.push(make(removed));
            out.push(make(added));
        }
        None => {}
    }
}

#[cfg(test)]
mod tests;
