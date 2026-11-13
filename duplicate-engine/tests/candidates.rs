use du_duplicate_engine::candidates::{
    self, EntryKind, Exclusion, FileIdentity, Placeholder, PrepareError, SampleRange, Size, Target,
};
use std::collections::BTreeSet;

fn target(item_no: u32, size: u64, file: Option<u8>) -> Target {
    Target {
        item_no,
        kind: EntryKind::File,
        size: Size::Exact(size),
        identity: file.map(|file| FileIdentity {
            volume: vec![1],
            file: vec![file],
        }),
        placeholder: Placeholder::Local,
    }
}

#[test]
fn size_groups_collapse_hardlinks_without_claiming_content_verification() -> Result<(), PrepareError>
{
    let input = [
        target(4, 10, Some(1)),
        target(2, 10, Some(1)),
        target(3, 10, Some(2)),
        target(5, 100, Some(3)),
        target(6, 0, Some(4)),
        target(7, 0, Some(5)),
    ];
    let result = candidates::prepare(&input)?;
    assert_eq!(
        result.groups.iter().map(|g| g.size).collect::<Vec<_>>(),
        vec![10, 0]
    );
    assert_eq!(result.groups[0].files[0].item_nos, vec![2, 4]);
    assert_eq!(result.groups[0].files[1].item_nos, vec![3]);
    assert_eq!(result.unpaired[0].item_nos, vec![5]);
    assert!(result.excluded.is_empty());
    let mut reversed = input.to_vec();
    reversed.reverse();
    assert_eq!(result, candidates::prepare(&reversed)?);
    Ok(())
}

#[test]
fn unknown_or_inconsistent_observations_are_not_promoted() -> Result<(), PrepareError> {
    let mut unknown_kind = target(1, 1, Some(1));
    unknown_kind.kind = EntryKind::Unknown;
    let mut non_file = target(2, 1, Some(2));
    non_file.kind = EntryKind::Other;
    let mut lower = target(3, 1, Some(3));
    lower.size = Size::LowerBound(1);
    let mut missing = target(4, 1, Some(4));
    missing.size = Size::Unknown;
    let mut cloud = target(5, 1, Some(5));
    cloud.placeholder = Placeholder::DownloadRequired;
    let mut uncertain = target(6, 1, None);
    uncertain.placeholder = Placeholder::Unknown;
    let input = [
        unknown_kind,
        non_file,
        lower,
        missing,
        cloud,
        uncertain,
        target(7, 1, None),
        target(8, 12, Some(9)),
        target(9, 13, Some(9)),
    ];
    let result = candidates::prepare(&input)?;
    assert_eq!(
        result
            .excluded
            .iter()
            .map(|e| (e.item_no, e.reason))
            .collect::<Vec<_>>(),
        vec![
            (1, Exclusion::KindUnknown),
            (2, Exclusion::NotRegularFile),
            (3, Exclusion::SizeNotExact),
            (4, Exclusion::SizeNotExact),
            (5, Exclusion::PlaceholderDownloadRequired),
            (8, Exclusion::IdentityConflict),
            (9, Exclusion::IdentityConflict),
        ]
    );
    assert_eq!(result.groups.len(), 1);
    assert!(result.groups[0].files[0].placeholder_unknown);
    assert!(result.groups[0].files.iter().all(|f| f.identity.is_none()));
    Ok(())
}

#[test]
fn malformed_identity_and_repeated_item_are_rejected() {
    let a = target(1, 5, Some(1));
    assert_eq!(
        candidates::prepare(&[a.clone(), a]),
        Err(PrepareError::DuplicateItem)
    );
    let mut invalid = target(2, 5, Some(1));
    invalid.identity = Some(FileIdentity {
        volume: vec![],
        file: vec![1],
    });
    assert_eq!(
        candidates::prepare(&[invalid]),
        Err(PrepareError::InvalidIdentity)
    );
    assert_eq!(candidates::prepare(&[]).map(|p| p.groups.len()), Ok(0));
}

#[test]
fn aliases_alone_are_unpaired_and_volume_is_part_of_identity() -> Result<(), PrepareError> {
    let first = target(1, 0, Some(7));
    let alias = target(2, 0, Some(7));
    let aliases = candidates::prepare(&[first.clone(), alias.clone()])?;
    assert!(aliases.groups.is_empty());
    assert_eq!(aliases.unpaired[0].item_nos, vec![1, 2]);
    let mut other_volume = target(3, 0, Some(7));
    other_volume.identity = Some(FileIdentity {
        volume: vec![2],
        file: vec![7],
    });
    let both = candidates::prepare(&[first, alias, other_volume])?;
    assert_eq!(both.groups.len(), 1);
    assert_eq!(both.groups[0].files.len(), 2);
    Ok(())
}

#[test]
fn partial_sampling_starts_only_above_128_kib_and_does_not_overflow() {
    const K: u64 = 1024;
    assert_eq!(candidates::sample_ranges(0), None);
    assert_eq!(candidates::sample_ranges(128 * K), None);
    assert_eq!(
        candidates::sample_ranges(128 * K + 1),
        Some([
            SampleRange {
                offset: 0,
                length: 64 * K
            },
            SampleRange {
                offset: 64 * K + 1,
                length: 64 * K
            }
        ])
    );
    let ranges = candidates::sample_ranges(u64::MAX);
    assert_eq!(ranges.map(|r| r[1].offset + r[1].length), Some(u64::MAX));
}

#[test]
fn prepared_candidates_never_contain_items_outside_supplied_list() -> Result<(), PrepareError> {
    let selected = [target(1, 10, Some(1)), target(3, 10, Some(3))];
    let outside = target(2, 10, Some(2));
    let result = candidates::prepare(&selected)?;
    let actual: BTreeSet<_> = result
        .groups
        .iter()
        .flat_map(|group| &group.files)
        .chain(&result.unpaired)
        .flat_map(|file| &file.item_nos)
        .copied()
        .collect();
    let allowed: BTreeSet<_> = selected.iter().map(|item| item.item_no).collect();
    assert!(actual.is_subset(&allowed));
    assert!(!actual.contains(&outside.item_no));
    Ok(())
}
