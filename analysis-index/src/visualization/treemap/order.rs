//! Display order of admitted treemap siblings.
use super::{AdmissionError, AdmittedChild, check};
use crate::ResultVersion;
use du_core_contracts::result::ResultVersionRead;
use std::cmp::Ordering;

// Stable merge of lightweight row indexes gives a cancellation check per output.
pub(super) fn order(
    version: &ResultVersion,
    rows: Vec<AdmittedChild>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<AdmittedChild>, AdmissionError> {
    let mut indices: Vec<usize> = (0..rows.len()).collect();
    let mut scratch = indices.clone();
    let mut width = 1usize;
    while width < indices.len() {
        let mut start = 0usize;
        while start < indices.len() {
            let mid = start.saturating_add(width).min(indices.len());
            let end = mid.saturating_add(width).min(indices.len());
            let (mut left, mut right) = (start, mid);
            for dest in &mut scratch[start..end] {
                check(cancelled)?;
                let take_left = right == end
                    || (left < mid && before(version, &rows[indices[left]], &rows[indices[right]]));
                if take_left {
                    *dest = indices[left];
                    left += 1;
                } else {
                    *dest = indices[right];
                    right += 1;
                }
            }
            start = end;
        }
        std::mem::swap(&mut indices, &mut scratch);
        width = width.saturating_mul(2);
    }
    let mut slots: Vec<_> = rows.into_iter().map(Some).collect();
    let mut output = Vec::with_capacity(indices.len());
    for index in indices {
        check(cancelled)?;
        output.push(slots[index].take().ok_or(AdmissionError::NoAttribution)?);
    }
    Ok(output)
}

/// Display order of two admitted siblings: attributed bytes descending, then
/// the raw name bytes preserved by the index, compared bytewise ascending
/// (a proper prefix sorts first). Scope roots have no parent and sort by their
/// scope-root number instead; the index numbers roots first, in root order, so
/// that number equals their item number. The item number is the last
/// tie-break only so that the order is total; names are unique among siblings.
///
/// The order is for display only. It does not decide whether two rows are the
/// same item and does not map items between result versions. It uses no
/// display name, locale or case folding.
///
/// Because the comparison is on raw bytes, a Windows name (UTF-16LE bytes) is
/// not in human alphabetical order: U+0100 (bytes 00 01) sorts before "a"
/// (bytes 61 00). The rule exists only to make the order deterministic.
///
/// Two siblings with equal bytes compare by their own names alone. While their
/// sizes stay equal, a later version of the same build may insert new siblings
/// between them without changing their relative order.
pub(super) fn before(version: &ResultVersion, a: &AdmittedChild, b: &AdmittedChild) -> bool {
    let (left, right) = (a.child.item.item_no, b.child.item.item_no);
    b.bytes
        .cmp(&a.bytes)
        .then_with(|| sibling_order(version, left, right))
        .then_with(|| left.cmp(&right))
        .is_le()
}

pub(super) fn sibling_order(version: &ResultVersion, left: u32, right: u32) -> Ordering {
    match (
        version.items().get(left as usize),
        version.items().get(right as usize),
    ) {
        (Some(a), Some(b)) if a.parent.is_none() && b.parent.is_none() => version
            .root_indices
            .get(left as usize)
            .cmp(&version.root_indices.get(right as usize)),
        (Some(a), Some(b)) => a.name.cmp(&b.name),
        _ => Ordering::Equal,
    }
}
