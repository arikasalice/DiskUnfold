//! F08 `rank.change-delta` revision 1 over one pinned comparison result.
use crate::comparison::Change;
use du_core_contracts::result::ResultItemKind;

/// Published ranking strategy identifier.
pub const STRATEGY_ID: &str = "rank.change-delta";
/// Published strategy revision.
pub const STRATEGY_REVISION: u32 = 1;

/// Classification supplied by the comparison and move-evidence stages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Classified(Change),
    /// The caller has established a move, not merely a suspected move.
    ConfirmedMove,
}

/// Ranking evidence for one row of a pinned comparison result.
#[derive(Clone, Copy, Debug)]
pub struct Candidate<'a> {
    /// The pinned path-comparison key, not a display path.
    pub path_key: &'a [u8],
    pub kind: ResultItemKind,
    pub status: Status,
    /// Exact signed bytes; `None` also covers lower bounds and unknown values.
    pub delta: Option<i128>,
    /// True only when each corresponding folder aggregate exists and is complete.
    pub folder_before_complete: bool,
    pub folder_after_complete: bool,
}

/// Source row indices in rank order and the full eligible count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankList {
    pub row_indices: Vec<usize>,
    pub total_eligible: usize,
}

/// The four separate direction and item-level rankings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rankings {
    pub increase_files: RankList,
    pub decrease_files: RankList,
    pub increase_folders: RankList,
    pub decrease_folders: RankList,
}

/// Cancellation leaves no partial ranking available to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Cancelled,
}

/// Ranks all eligible rows without applying a display count or page limit.
///
/// Non-exact, zero, unverifiable and incomparable rows are excluded. Folder
/// candidates require complete evidence for both sides. Confirmed moves retain
/// their delta sign. Equal magnitudes use the raw comparison key; fully equal
/// keys retain input order. Cancellation is checked per row and per merge step;
/// one key comparison and one allocation are not interruptible.
pub fn rank(
    rows: &[Candidate<'_>],
    mut cancelled: impl FnMut() -> bool,
) -> Result<Rankings, Error> {
    let mut lists: [Vec<usize>; 4] = std::array::from_fn(|_| Vec::new());
    for (index, row) in rows.iter().enumerate() {
        check(&mut cancelled)?;
        if let Some(slot) = eligible(row) {
            lists[slot].push(index);
        }
    }
    for list in &mut lists {
        sort(list, rows, &mut cancelled)?;
    }
    check(&mut cancelled)?;
    let [a, b, c, d] = lists;
    Ok(Rankings {
        increase_files: wrap(a),
        decrease_files: wrap(b),
        increase_folders: wrap(c),
        decrease_folders: wrap(d),
    })
}

fn wrap(row_indices: Vec<usize>) -> RankList {
    RankList {
        total_eligible: row_indices.len(),
        row_indices,
    }
}
fn eligible(row: &Candidate<'_>) -> Option<usize> {
    let delta = row.delta?;
    let positive = match row.status {
        Status::Classified(Change::Added | Change::Grown) if delta > 0 => true,
        Status::Classified(Change::Removed | Change::Shrunk) if delta < 0 => false,
        Status::ConfirmedMove if delta > 0 => true,
        Status::ConfirmedMove if delta < 0 => false,
        _ => return None,
    };
    match row.kind {
        ResultItemKind::File => Some(usize::from(!positive)),
        ResultItemKind::Directory if row.folder_before_complete && row.folder_after_complete => {
            Some(2 + usize::from(!positive))
        }
        _ => None,
    }
}
fn check(cancelled: &mut impl FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

// Bottom-up stable merge sort gives cancellation checkpoints during sorting.
fn sort(
    indices: &mut Vec<usize>,
    rows: &[Candidate<'_>],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), Error> {
    let len = indices.len();
    let mut scratch = indices.clone();
    let mut width = 1_usize;
    while width < len {
        let mut start = 0;
        while start < len {
            let middle = start.saturating_add(width).min(len);
            let end = middle.saturating_add(width).min(len);
            merge(indices, &mut scratch, rows, start, middle, end, cancelled)?;
            start = end;
        }
        std::mem::swap(indices, &mut scratch);
        width = width.saturating_mul(2);
    }
    Ok(())
}
fn merge(
    source: &[usize],
    dest: &mut [usize],
    rows: &[Candidate<'_>],
    start: usize,
    middle: usize,
    end: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), Error> {
    let (mut left, mut right) = (start, middle);
    for output in &mut dest[start..end] {
        check(cancelled)?;
        let take_left =
            right == end || (left < middle && before(rows, source[left], source[right]));
        *output = if take_left {
            let index = source[left];
            left += 1;
            index
        } else {
            let index = source[right];
            right += 1;
            index
        };
    }
    Ok(())
}
fn before(rows: &[Candidate<'_>], left: usize, right: usize) -> bool {
    let l = rows[left].delta.map(i128::unsigned_abs).unwrap_or(0);
    let r = rows[right].delta.map(i128::unsigned_abs).unwrap_or(0);
    l > r || (l == r && rows[left].path_key <= rows[right].path_key)
}

#[cfg(test)]
mod tests;
