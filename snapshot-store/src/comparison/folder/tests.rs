use super::*;
use crate::{
    records::NameEncoding,
    storage_match::{Capability, Observation},
};

fn raw(number: u32, parent: u32, name: &[u8], kind: Kind, bytes: Size) -> Vec<u8> {
    Item {
        number,
        parent,
        kind,
        flags: 0,
        name,
        sizes: [bytes, Size::Unknown, Size::Unknown],
        modified_ms: None,
        identity: &[],
        link_count: 0,
    }
    .encode(NameEncoding::Bytes)
    .unwrap_or_default()
}
fn root(same: bool) -> RootComparison {
    let before = Observation {
        capability: Capability::Supported,
        kind: Some(b"id"),
        value: Some(b"one"),
    };
    let after = Observation {
        value: Some(if same { b"one" } else { b"two" }),
        ..before
    };
    RootComparison::new(before, after)
}
fn decode(bytes: &[u8]) -> Result<Item<'_>, String> {
    crate::records::item::decode(bytes, NameEncoding::Bytes).map_err(|e| format!("{e:?}"))
}
fn observed(_: &[u8]) -> Location {
    Location::Comparable { after_gap: None }
}

#[test]
fn actual_decoded_children_merge_and_preserve_raw_observations() -> Result<(), String> {
    let a = raw(1, 0, b"a", Kind::File, Size::Exact(10));
    let b = raw(2, 0, b"b", Kind::File, Size::Exact(20));
    let b2 = raw(4, 0, b"b", Kind::File, Size::Exact(25));
    let c = raw(5, 0, b"c", Kind::Folder, Size::Unknown);
    let (a, b, b2, c) = (decode(&a)?, decode(&b)?, decode(&b2)?, decode(&c)?);
    let left = [
        Child {
            item: &b,
            key: b"b",
        },
        Child {
            item: &a,
            key: b"a",
        },
    ];
    let right = [
        Child {
            item: &c,
            key: b"c",
        },
        Child {
            item: &b2,
            key: b"b",
        },
    ];
    let rows = merge(
        &root(true),
        (0, 0),
        &left,
        &right,
        &SizeBasis::Logical,
        observed,
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        rows.iter().map(|row| row.key).collect::<Vec<_>>(),
        [b"a".as_slice(), b"b", b"c"]
    );
    assert_eq!(
        rows.iter().map(|row| row.change).collect::<Vec<_>>(),
        [Change::Removed, Change::Grown, Change::Added]
    );
    assert_eq!(
        rows.iter().map(|row| row.delta).collect::<Vec<_>>(),
        [Some(-10), Some(5), None]
    );
    assert_eq!(rows[2].after, Some(&c));
    let reversed = merge(
        &root(true),
        (0, 0),
        &[left[1], left[0]],
        &[right[1], right[0]],
        &SizeBasis::Logical,
        observed,
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(rows, reversed);
    Ok(())
}

#[test]
fn collisions_and_range_evidence_never_create_false_changes() -> Result<(), String> {
    let a = raw(1, 0, b"A", Kind::File, Size::Exact(1));
    let b = raw(2, 0, b"a", Kind::File, Size::Exact(2));
    let c = raw(3, 0, b"a", Kind::File, Size::Exact(3));
    let (a, b, c) = (decode(&a)?, decode(&b)?, decode(&c)?);
    let left = [
        Child {
            item: &b,
            key: b"a",
        },
        Child {
            item: &a,
            key: b"a",
        },
    ];
    let right = [Child {
        item: &c,
        key: b"a",
    }];
    let rows = merge(
        &root(true),
        (0, 0),
        &left,
        &right,
        &SizeBasis::Logical,
        observed,
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.change == Change::Incomparable(Gap::KeyCollision) && row.delta.is_none()));
    assert_eq!(rows[0].before.map(|item| item.name), Some(b"A".as_slice()));
    let unavailable = merge(
        &root(true),
        (0, 0),
        &left,
        &right,
        &SizeBasis::Logical,
        |_| Location::Incomparable(Gap::Excluded),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(
        unavailable
            .iter()
            .all(|row| row.change == Change::Incomparable(Gap::Excluded))
    );
    let different = merge(
        &root(false),
        (0, 0),
        &left,
        &right,
        &SizeBasis::Logical,
        observed,
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(
        different
            .iter()
            .all(|row| row.change == Change::Incomparable(Gap::DifferentStorage))
    );
    Ok(())
}

#[test]
fn missing_later_parent_and_type_changes_are_conservative() -> Result<(), String> {
    let one = raw(1, 0, b"a", Kind::File, Size::Exact(5));
    let two = raw(2, 0, b"a", Kind::Folder, Size::Exact(8));
    let (one, two) = (decode(&one)?, decode(&two)?);
    let left = [Child {
        item: &one,
        key: b"a",
    }];
    let right = [Child {
        item: &two,
        key: b"a",
    }];
    let missing = merge(
        &root(true),
        (0, 0),
        &left,
        &[],
        &SizeBasis::Logical,
        |_| Location::Comparable {
            after_gap: Some(Gap::MissingObservation),
        },
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (missing[0].change, missing[0].delta),
        (Change::Unverified(Gap::MissingObservation), None)
    );
    let changed = merge(
        &root(true),
        (0, 0),
        &left,
        &right,
        &SizeBasis::Logical,
        observed,
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        changed.iter().map(|row| row.change).collect::<Vec<_>>(),
        [Change::Removed, Change::Added]
    );
    Ok(())
}

#[test]
fn cancellation_and_wrong_parent_return_no_partial_rows() -> Result<(), String> {
    let a = raw(1, 0, b"a", Kind::File, Size::Exact(5));
    let a = decode(&a)?;
    let children = [Child {
        item: &a,
        key: b"a",
    }];
    assert!(matches!(
        merge(
            &root(true),
            (1, 0),
            &children,
            &[],
            &SizeBasis::Logical,
            observed,
            || false
        ),
        Err(Error::WrongParent)
    ));
    for limit in [1, 4, 7] {
        let mut checks = 0;
        let result = merge(
            &root(true),
            (0, 0),
            &children,
            &[],
            &SizeBasis::Logical,
            observed,
            || {
                checks += 1;
                checks >= limit
            },
        );
        assert!(matches!(result, Err(Error::Cancelled)), "limit {limit}");
    }
    Ok(())
}
