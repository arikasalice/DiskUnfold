use super::*;

#[test]
fn a_deep_range_can_be_resolved_and_released_iteratively() -> Result<(), tree::Error> {
    let path = vec![vec![b'd']; 20_000];
    let ranges = Ranges::new(
        &[Evidence::Range {
            path: &path,
            side: Side::Earlier,
            reason: RangeReason::Excluded,
        }],
        || false,
    )?;
    assert_eq!(
        ranges.location(&path),
        Location::Incomparable(Gap::Excluded)
    );
    drop(ranges);
    Ok(())
}
use crate::{
    comparison::{Change, folder::Child},
    records::item::{Item, Kind, Size},
    storage_match::{Capability, Observation},
};

fn path(parts: &[&[u8]]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.to_vec()).collect()
}
fn item(number: u32, parent: u32, kind: Kind, name: &'static [u8]) -> Item<'static> {
    Item {
        number,
        parent,
        kind,
        flags: 0,
        name,
        sizes: [Size::Exact(1), Size::Unknown, Size::Unknown],
        modified_ms: None,
        identity: &[],
        link_count: 0,
    }
}
fn root(after: Option<&[u8]>) -> RootComparison {
    let before = Observation {
        capability: Capability::Supported,
        kind: Some(b"id"),
        value: Some(b"a"),
    };
    RootComparison::new(
        before,
        Observation {
            value: after,
            ..before
        },
    )
}

#[test]
fn prefix_priority_and_parent_observation_are_component_scoped() -> Result<(), tree::Error> {
    let a = path(&[b"a"]);
    let child = path(&[b"a", b"child"]);
    let excluded = path(&[b"a", b"excluded"]);
    let entries = [
        Evidence::LaterFolder {
            path: &a,
            reason: Incomplete::AccessDenied,
        },
        Evidence::Range {
            path: &excluded,
            side: Side::Earlier,
            reason: RangeReason::MissingObservation,
        },
        Evidence::Range {
            path: &excluded,
            side: Side::Later,
            reason: RangeReason::Excluded,
        },
        Evidence::Range {
            path: &excluded,
            side: Side::Earlier,
            reason: RangeReason::OutsideScope,
        },
    ];
    let ranges = Ranges::new(&entries, || false)?;
    assert_eq!(
        ranges.location(&a),
        Location::Comparable { after_gap: None }
    );
    assert_eq!(
        ranges.location(&child),
        Location::Comparable {
            after_gap: Some(Gap::AccessDenied)
        }
    );
    assert_eq!(
        ranges.location(&path(&[b"a", b"child", b"grandchild"])),
        Location::Comparable { after_gap: None }
    );
    assert_eq!(
        ranges.location(&path(&[b"ab", b"child"])),
        Location::Comparable { after_gap: None }
    );
    assert_eq!(
        ranges.location(&path(&[b"a", b"excluded", b"nested"])),
        Location::Incomparable(Gap::OutsideScope)
    );
    Ok(())
}

#[test]
fn assembler_uses_index_and_storage_boundary() -> Result<(), tree::Error> {
    let folder = item(1, 0, Kind::Folder, b"a");
    let child = item(2, 1, Kind::File, b"child");
    let sibling = item(3, 0, Kind::File, b"b");
    let before = tree::Tree::new(
        &[
            Child {
                item: &folder,
                key: b"a",
            },
            Child {
                item: &child,
                key: b"child",
            },
            Child {
                item: &sibling,
                key: b"b",
            },
        ],
        || false,
    )?;
    let after = tree::Tree::new(&[], || false)?;
    let root_path = path(&[]);
    let ranges = Ranges::new(
        &[Evidence::LaterFolder {
            path: &root_path,
            reason: Incomplete::MissingObservation,
        }],
        || false,
    )?;
    let rows = compare(
        &root(Some(b"a")),
        &before,
        &after,
        &SizeBasis::Logical,
        &ranges,
        || false,
    )?;
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|row| row.change == Change::Unverified(Gap::MissingObservation))
    );
    let rows = compare(
        &root(Some(b"b")),
        &before,
        &after,
        &SizeBasis::Logical,
        &ranges,
        || false,
    )?;
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.change == Change::Incomparable(Gap::DifferentStorage))
    );
    let rows = compare(
        &root(None),
        &before,
        &after,
        &SizeBasis::Logical,
        &ranges,
        || false,
    )?;
    assert!(
        rows.iter()
            .all(|row| row.change == Change::Incomparable(Gap::UnknownStorage))
    );
    let a = path(&[b"a"]);
    let ranges = Ranges::new(
        &[Evidence::Range {
            path: &a,
            side: Side::Earlier,
            reason: RangeReason::Excluded,
        }],
        || false,
    )?;
    let rows = compare(
        &root(Some(b"a")),
        &before,
        &after,
        &SizeBasis::Logical,
        &ranges,
        || false,
    )?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].change, Change::Incomparable(Gap::Excluded));
    assert_eq!(rows[1].change, Change::Removed);
    Ok(())
}

#[test]
fn cancellation_discards_range_and_comparison_results() -> Result<(), tree::Error> {
    let a = path(&[b"a", b"child"]);
    let mut calls = 0;
    assert!(matches!(
        Ranges::new(
            &[Evidence::Range {
                path: &a,
                side: Side::Later,
                reason: RangeReason::Excluded
            }],
            || {
                calls += 1;
                calls == 2
            }
        ),
        Err(tree::Error::Cancelled)
    ));
    let ranges = Ranges::new(&[], || false)?;
    let empty = tree::Tree::new(&[], || false)?;
    assert!(matches!(
        compare(
            &root(Some(b"a")),
            &empty,
            &empty,
            &SizeBasis::Logical,
            &ranges,
            || true
        ),
        Err(tree::Error::Cancelled)
    ));
    Ok(())
}
