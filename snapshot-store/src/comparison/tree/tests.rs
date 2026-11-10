use super::*;
use crate::{
    comparison::folder::Child,
    records::item::Size,
    storage_match::{Capability, Observation},
};
fn item(number: u32, parent: u32, kind: Kind, name: &'static [u8], bytes: u64) -> Item<'static> {
    Item {
        number,
        parent,
        kind,
        flags: 0,
        name,
        sizes: [Size::Exact(bytes), Size::Unknown, Size::Unknown],
        modified_ms: None,
        identity: &[],
        link_count: 0,
    }
}
fn root(same: bool) -> RootComparison {
    let a = Observation {
        capability: Capability::Supported,
        kind: Some(b"id"),
        value: Some(b"a"),
    };
    RootComparison::new(
        a,
        Observation {
            value: Some(if same { b"a" } else { b"b" }),
            ..a
        },
    )
}
fn observed(_: &[Vec<u8>]) -> Location {
    Location::Comparable { after_gap: None }
}
fn names(rows: &[Row<'_>]) -> Vec<Vec<Vec<u8>>> {
    rows.iter().map(|row| row.path.clone()).collect()
}

#[test]
fn two_trees_merge_depth_first_and_input_order_does_not_change_result() -> Result<(), Error> {
    let a = item(1, 0, Kind::Folder, b"a", 10);
    let inner = item(2, 1, Kind::File, b"child", 10);
    let b = item(3, 0, Kind::File, b"b", 7);
    let a2 = item(11, 0, Kind::Folder, b"a", 12);
    let inner2 = item(12, 11, Kind::File, b"child", 12);
    let c2 = item(13, 0, Kind::File, b"c", 3);
    let left = [
        Child {
            item: &b,
            key: b"b",
        },
        Child {
            item: &inner,
            key: b"child",
        },
        Child {
            item: &a,
            key: b"a",
        },
    ];
    let right = [
        Child {
            item: &inner2,
            key: b"child",
        },
        Child {
            item: &c2,
            key: b"c",
        },
        Child {
            item: &a2,
            key: b"a",
        },
    ];
    let left_tree = Tree::new(&left, || false)?;
    let right_tree = Tree::new(&right, || false)?;
    let result = compare(
        &root(true),
        &left_tree,
        &right_tree,
        &SizeBasis::Logical,
        observed,
        || false,
    )?;
    assert_eq!(
        names(&result),
        [
            vec![b"a".to_vec()],
            vec![b"a".to_vec(), b"child".to_vec()],
            vec![b"b".to_vec()],
            vec![b"c".to_vec()]
        ]
    );
    assert_eq!(
        result.iter().map(|r| r.change).collect::<Vec<_>>(),
        [Change::Grown, Change::Grown, Change::Removed, Change::Added]
    );
    let other = Tree::new(&[left[2], left[0], left[1]], || false)?;
    assert_eq!(
        result,
        compare(
            &root(true),
            &other,
            &right_tree,
            &SizeBasis::Logical,
            observed,
            || false
        )?
    );
    Ok(())
}

#[test]
fn incomparable_and_colliding_folders_are_never_descended() -> Result<(), Error> {
    let folder = item(1, 0, Kind::Folder, b"A", 9);
    let collision = item(2, 0, Kind::Folder, b"a", 9);
    let hidden = item(3, 1, Kind::File, b"hidden", 5);
    let left = Tree::new(
        &[
            Child {
                item: &folder,
                key: b"a",
            },
            Child {
                item: &collision,
                key: b"a",
            },
            Child {
                item: &hidden,
                key: b"hidden",
            },
        ],
        || false,
    )?;
    let empty = Tree::new(&[], || false)?;
    let rows = compare(
        &root(true),
        &left,
        &empty,
        &SizeBasis::Logical,
        observed,
        || false,
    )?;
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.change == Change::Incomparable(Gap::KeyCollision))
    );
    let mut visited = Vec::new();
    let rows = compare(
        &root(false),
        &left,
        &empty,
        &SizeBasis::Logical,
        |path| {
            visited.push(path.to_vec());
            observed(path)
        },
        || false,
    )?;
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.change == Change::Incomparable(Gap::DifferentStorage))
    );
    assert!(visited.iter().all(|path| path.len() == 1));
    Ok(())
}

#[test]
fn missing_parent_gap_is_inherited_without_turning_descendants_into_deletions() -> Result<(), Error>
{
    let folder = item(1, 0, Kind::Folder, b"a", 9);
    let child = item(2, 1, Kind::File, b"child", 5);
    let left = Tree::new(
        &[
            Child {
                item: &folder,
                key: b"a",
            },
            Child {
                item: &child,
                key: b"child",
            },
        ],
        || false,
    )?;
    let empty = Tree::new(&[], || false)?;
    let rows = compare(
        &root(true),
        &left,
        &empty,
        &SizeBasis::Logical,
        |path| {
            if path.len() == 1 {
                Location::Comparable {
                    after_gap: Some(Gap::AccessDenied),
                }
            } else {
                observed(path)
            }
        },
        || false,
    )?;
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.change == Change::Unverified(Gap::AccessDenied) && row.delta.is_none())
    );
    let stronger = compare(
        &root(true),
        &left,
        &empty,
        &SizeBasis::Logical,
        |path| Location::Comparable {
            after_gap: Some(if path.len() == 1 {
                Gap::AccessDenied
            } else {
                Gap::Excluded
            }),
        },
        || false,
    )?;
    assert_eq!(stronger[1].change, Change::Unverified(Gap::Excluded));
    let excluded = compare(
        &root(true),
        &left,
        &empty,
        &SizeBasis::Logical,
        |_| Location::Incomparable(Gap::Excluded),
        || false,
    )?;
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded[0].change, Change::Incomparable(Gap::Excluded));
    let removed = compare(
        &root(true),
        &left,
        &empty,
        &SizeBasis::Logical,
        observed,
        || false,
    )?;
    assert!(removed.iter().all(|row| row.change == Change::Removed));
    let added = compare(
        &root(true),
        &empty,
        &left,
        &SizeBasis::Logical,
        observed,
        || false,
    )?;
    assert!(added.iter().all(|row| row.change == Change::Added));
    Ok(())
}

#[test]
fn malformed_trees_and_cancellation_fail_without_partial_output() -> Result<(), Error> {
    let folder = item(1, 0, Kind::Folder, b"a", 9);
    let bad = item(2, 7, Kind::File, b"bad", 1);
    let invalid_parent = [
        Child {
            item: &folder,
            key: b"a",
        },
        Child {
            item: &bad,
            key: b"bad",
        },
    ];
    assert!(matches!(
        Tree::new(&invalid_parent, || false),
        Err(Error::InvalidTree)
    ));
    let duplicate = [
        Child {
            item: &folder,
            key: b"a",
        },
        Child {
            item: &folder,
            key: b"a",
        },
    ];
    assert!(matches!(
        Tree::new(&duplicate, || false),
        Err(Error::InvalidTree)
    ));
    let cyclic = item(3, 3, Kind::Folder, b"cycle", 1);
    assert!(matches!(
        Tree::new(
            &[Child {
                item: &cyclic,
                key: b"cycle"
            }],
            || false
        ),
        Err(Error::InvalidTree)
    ));
    let left = Tree::new(
        &[Child {
            item: &folder,
            key: b"a",
        }],
        || false,
    )?;
    let empty = Tree::new(&[], || false)?;
    let mut calls = 0;
    assert!(matches!(
        compare(
            &root(true),
            &left,
            &empty,
            &SizeBasis::Logical,
            observed,
            || {
                calls += 1;
                calls >= 5
            }
        ),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        Tree::new(&duplicate, || true),
        Err(Error::Cancelled)
    ));
    Ok(())
}
