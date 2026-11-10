//! Stable ordering for interruptible visualization calculations.

use std::cmp::Ordering;

/// Merges bounded chunks and checks cancellation before each output element.
/// The caller owns the scratchable input and discards it if cancelled.
pub(super) fn sort<T: Copy>(
    values: &mut Vec<T>,
    compare: impl Fn(&T, &T) -> Ordering,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), ()> {
    let mut scratch = Vec::with_capacity(values.len());
    for value in values.iter() {
        if cancelled() {
            return Err(());
        }
        scratch.push(*value);
    }
    let mut width = 1usize;
    while width < values.len() {
        let mut start = 0usize;
        while start < values.len() {
            let mid = start.saturating_add(width).min(values.len());
            let end = mid.saturating_add(width).min(values.len());
            let (mut left, mut right) = (start, mid);
            for slot in &mut scratch[start..end] {
                if cancelled() {
                    return Err(());
                }
                if right == end
                    || (left < mid && compare(&values[left], &values[right]) != Ordering::Greater)
                {
                    *slot = values[left];
                    left += 1;
                } else {
                    *slot = values[right];
                    right += 1;
                }
            }
            start = end;
        }
        std::mem::swap(values, &mut scratch);
        width = width.saturating_mul(2);
    }
    if cancelled() { Err(()) } else { Ok(()) }
}
