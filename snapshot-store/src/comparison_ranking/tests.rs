use super::*;
use crate::comparison::Gap;

fn candidate<'a>(
    path: &'a [u8],
    kind: ResultItemKind,
    status: Status,
    delta: Option<i128>,
) -> Candidate<'a> {
    Candidate {
        path_key: path,
        kind,
        status,
        delta,
        folder_before_complete: true,
        folder_after_complete: true,
    }
}

#[test]
fn separates_four_rankings_and_includes_confirmed_moves() -> Result<(), Error> {
    use ResultItemKind::{Directory as Folder, File};
    let rows = [
        candidate(b"b", File, Status::Classified(Change::Added), Some(100)),
        candidate(b"a", File, Status::Classified(Change::Grown), Some(100)),
        candidate(b"move", File, Status::ConfirmedMove, Some(120)),
        candidate(
            b"deleted",
            File,
            Status::Classified(Change::Removed),
            Some(-12),
        ),
        candidate(
            b"shrink",
            File,
            Status::Classified(Change::Shrunk),
            Some(-20),
        ),
        candidate(b"moved", File, Status::ConfirmedMove, Some(-30)),
        candidate(b"dir", Folder, Status::Classified(Change::Grown), Some(55)),
        candidate(b"dirmove", Folder, Status::ConfirmedMove, Some(-70)),
    ];
    let ranked = rank(&rows, || false)?;
    assert_eq!(ranked.increase_files.row_indices, vec![2, 1, 0]);
    assert_eq!(ranked.decrease_files.row_indices, vec![5, 4, 3]);
    assert_eq!(ranked.increase_folders.row_indices, vec![6]);
    assert_eq!(ranked.decrease_folders.row_indices, vec![7]);
    assert_eq!(ranked.increase_files.total_eligible, 3);
    assert_eq!(ranked.decrease_folders.total_eligible, 1);
    Ok(())
}

#[test]
fn excludes_unconfirmed_unknown_zero_inconsistent_and_incomplete_folders() -> Result<(), Error> {
    use ResultItemKind::{Directory as Folder, File, Unknown};
    let mut partial_before = candidate(b"p", Folder, Status::Classified(Change::Grown), Some(8));
    partial_before.folder_before_complete = false;
    let mut partial_after = candidate(b"q", Folder, Status::ConfirmedMove, Some(-8));
    partial_after.folder_after_complete = false;
    let rows = [
        candidate(b"a", File, Status::Classified(Change::Added), None),
        candidate(b"z", File, Status::Classified(Change::Unchanged), Some(0)),
        candidate(
            b"u",
            File,
            Status::Classified(Change::Unverified(Gap::MissingObservation)),
            Some(9),
        ),
        candidate(
            b"i",
            File,
            Status::Classified(Change::Incomparable(Gap::DifferentStorage)),
            Some(-9),
        ),
        candidate(
            b"s",
            File,
            Status::Classified(Change::SizeUnverified),
            Some(9),
        ),
        candidate(b"bad", File, Status::Classified(Change::Added), Some(-2)),
        candidate(b"x", Unknown, Status::ConfirmedMove, Some(3)),
        partial_before,
        partial_after,
    ];
    let ranked = rank(&rows, || false)?;
    for list in [
        ranked.increase_files,
        ranked.decrease_files,
        ranked.increase_folders,
        ranked.decrease_folders,
    ] {
        assert_eq!(list.total_eligible, 0);
        assert!(list.row_indices.is_empty());
    }
    Ok(())
}

#[test]
fn exact_extremes_binary_keys_and_equal_keys_are_stable() -> Result<(), Error> {
    use ResultItemKind::File;
    let rows = [
        candidate(&[255], File, Status::ConfirmedMove, Some(i128::MIN)),
        candidate(
            &[0, 255],
            File,
            Status::Classified(Change::Removed),
            Some(-10),
        ),
        candidate(&[0, 0], File, Status::Classified(Change::Shrunk), Some(-10)),
        candidate(&[0, 0], File, Status::ConfirmedMove, Some(-10)),
    ];
    assert_eq!(
        rank(&rows, || false)?.decrease_files.row_indices,
        vec![0, 2, 3, 1]
    );
    Ok(())
}

#[test]
fn cancellation_before_during_scan_and_during_sort_returns_no_result() {
    let rows = vec![
        candidate(
            b"same",
            ResultItemKind::File,
            Status::ConfirmedMove,
            Some(1)
        );
        128
    ];
    assert_eq!(rank(&rows, || true), Err(Error::Cancelled));
    let mut calls = 0;
    assert_eq!(
        rank(&rows, || {
            calls += 1;
            calls == 20
        }),
        Err(Error::Cancelled)
    );
    calls = 0;
    assert_eq!(
        rank(&rows, || {
            calls += 1;
            calls == 145
        }),
        Err(Error::Cancelled)
    );
    assert_eq!(calls, 145);
}

#[test]
fn many_rows_match_magnitude_and_key_reference_order() -> Result<(), Error> {
    let keys: Vec<Vec<u8>> = (0..257u32)
        .map(|n| (256 - n).to_be_bytes().to_vec())
        .collect();
    let rows: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(n, k)| {
            candidate(
                k,
                ResultItemKind::File,
                Status::ConfirmedMove,
                Some(1 + (n % 13) as i128),
            )
        })
        .collect();
    let mut expected: Vec<_> = (0..rows.len()).collect();
    expected.sort_by(|a, b| {
        rows[*b]
            .delta
            .cmp(&rows[*a].delta)
            .then_with(|| rows[*a].path_key.cmp(rows[*b].path_key))
    });
    assert_eq!(rank(&rows, || false)?.increase_files.row_indices, expected);
    Ok(())
}
