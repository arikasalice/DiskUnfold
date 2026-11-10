use super::*;
use du_core_contracts::SizeEvidence;

const OBSERVED: Location = Location::Comparable { after_gap: None };

fn size(amount: SizeAmount) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        amount,
        evidence: SizeEvidence::OsReported,
    }
}

fn exact_size(bytes: u64) -> SizeValue {
    size(SizeAmount::Exact {
        bytes: bytes.into(),
    })
}

fn file(item_no: u32, size: &SizeValue) -> Side<'_> {
    Side {
        item_no,
        kind: ResultItemKind::File,
        size: Some(size),
    }
}

fn classify_row<'a>(
    before: Option<Side<'a>>,
    after: Option<Side<'a>>,
    location: Location,
) -> Result<Row<'a>, &'static str> {
    match classify(before, after, location, &SizeBasis::Logical) {
        Some(Rows::One(row)) => Ok(row),
        _ => Err("expected a single classification row"),
    }
}

#[test]
fn classifies_added_removed_and_paired_changes() -> Result<(), &'static str> {
    let a = exact_size(100);
    let b_before = exact_size(200);
    let b_after = exact_size(250);
    let c = exact_size(30);
    let removed = classify_row(Some(file(1, &a)), None, OBSERVED)?;
    let grown = classify_row(Some(file(2, &b_before)), Some(file(1, &b_after)), OBSERVED)?;
    let added = classify_row(None, Some(file(2, &c)), OBSERVED)?;
    assert_eq!(
        (removed.change, removed.delta),
        (Change::Removed, Some(-100))
    );
    assert_eq!((grown.change, grown.delta), (Change::Grown, Some(50)));
    assert_eq!((added.change, added.delta), (Change::Added, Some(30)));
    let shrunk = classify_row(Some(file(1, &b_after)), Some(file(2, &b_before)), OBSERVED)?;
    assert_eq!((shrunk.change, shrunk.delta), (Change::Shrunk, Some(-50)));
    Ok(())
}

#[test]
fn exact_zero_and_non_exact_values_remain_distinct() -> Result<(), &'static str> {
    let zero = exact_size(0);
    let lower = size(SizeAmount::LowerBound { bytes: 0.into() });
    let unknown = size(SizeAmount::Unknown);
    let zero = file(1, &zero);
    let same = classify_row(Some(zero), Some(zero), OBSERVED)?;
    assert_eq!((same.change, same.delta), (Change::Unchanged, Some(0)));
    for size in [&lower, &unknown] {
        let non_exact = file(2, size);
        let paired = classify_row(Some(zero), Some(non_exact), OBSERVED)?;
        assert_eq!(
            (paired.change, paired.delta),
            (Change::SizeUnverified, None)
        );
        assert_eq!(paired.after, Some(non_exact));
        let added = classify_row(None, Some(non_exact), OBSERVED)?;
        assert_eq!((added.change, added.delta), (Change::Added, None));
    }
    Ok(())
}

#[test]
fn unseen_location_never_establishes_removal() -> Result<(), &'static str> {
    let exact = exact_size(15);
    let before = file(1, &exact);
    let row = classify_row(
        Some(before),
        None,
        Location::Comparable {
            after_gap: Some(Gap::AccessDenied),
        },
    )?;
    assert_eq!(row.change, Change::Unverified(Gap::AccessDenied));
    assert_eq!(row.before, Some(before));
    assert_eq!(row.delta, None);
    Ok(())
}

#[test]
fn classifies_replacement_after_range_and_collision_checks() -> Result<(), &'static str> {
    let size = exact_size(10);
    let before = file(1, &size);
    let after = Side {
        item_no: 2,
        kind: ResultItemKind::Directory,
        size: Some(&size),
    };
    for gap in [Gap::KeyCollision, Gap::DifferentStorage] {
        let row = classify_row(Some(before), Some(after), Location::Incomparable(gap))?;
        assert_eq!(row.change, Change::Incomparable(gap));
        assert_eq!(row.delta, None);
    }
    let Some(Rows::TypeChanged { removed, added }) =
        classify(Some(before), Some(after), OBSERVED, &SizeBasis::Logical)
    else {
        return Err("expected two ordered replacement rows");
    };
    assert_eq!((removed.before, removed.after), (Some(before), None));
    assert_eq!((added.before, added.after), (None, Some(after)));
    assert_eq!((removed.delta, added.delta), (Some(-10), Some(10)));
    Ok(())
}

#[test]
fn signed_delta_covers_the_full_byte_size_range() -> Result<(), &'static str> {
    let max = exact_size(u64::MAX);
    let zero = exact_size(0);
    let shrink = classify_row(Some(file(1, &max)), Some(file(1, &zero)), OBSERVED)?;
    assert_eq!(shrink.delta, Some(-i128::from(u64::MAX)));
    let other_basis = SizeValue {
        basis: SizeBasis::Allocated,
        ..max
    };
    let mismatch = classify_row(Some(file(1, &zero)), Some(file(1, &other_basis)), OBSERVED)?;
    assert_eq!(
        (mismatch.change, mismatch.delta),
        (Change::SizeUnverified, None)
    );
    Ok(())
}

#[test]
fn root_storage_decision_controls_existing_change_classification() -> Result<(), &'static str> {
    use crate::storage_match::{Capability, Correspondence, Observation};
    let observed = |kind, value| Observation {
        capability: Capability::Supported,
        kind: Some(kind),
        value: Some(value),
    };
    let exact = exact_size(10);
    let before = file(1, &exact);
    let same = RootComparison::new(observed(b"volume", b"a"), observed(b"volume", b"a"));
    assert_eq!(same.correspondence(), Correspondence::Same);
    let Some(Rows::One(removed)) = same.classify(Some(before), None, OBSERVED, &SizeBasis::Logical)
    else {
        return Err("expected removal");
    };
    assert_eq!(
        (removed.change, removed.delta),
        (Change::Removed, Some(-10))
    );
    let Some(Rows::One(unverified)) = same.classify(
        Some(before),
        None,
        Location::Comparable {
            after_gap: Some(Gap::AccessDenied),
        },
        &SizeBasis::Logical,
    ) else {
        return Err("expected unverified row");
    };
    assert_eq!(
        (unverified.change, unverified.delta),
        (Change::Unverified(Gap::AccessDenied), None)
    );
    for (later, expected) in [
        (observed(b"volume", b"b"), Gap::DifferentStorage),
        (observed(b"other-kind", b"a"), Gap::UnknownStorage),
        (
            Observation {
                capability: Capability::Partial,
                kind: Some(b"volume"),
                value: Some(b"a"),
            },
            Gap::UnknownStorage,
        ),
        (
            Observation {
                capability: Capability::Supported,
                kind: None,
                value: None,
            },
            Gap::UnknownStorage,
        ),
    ] {
        let comparison = RootComparison::new(observed(b"volume", b"a"), later);
        let Some(Rows::One(row)) =
            comparison.classify(Some(before), None, OBSERVED, &SizeBasis::Logical)
        else {
            return Err("expected incomparable row");
        };
        assert_eq!(
            (row.change, row.delta, row.before),
            (Change::Incomparable(expected), None, Some(before))
        );
    }
    Ok(())
}

#[test]
fn storage_rejection_takes_precedence_over_item_and_range_evidence() -> Result<(), &'static str> {
    use crate::storage_match::{Capability, Observation};
    let left = Observation {
        capability: Capability::Supported,
        kind: Some(b"id"),
        value: Some(b"a"),
    };
    let other = Observation {
        value: Some(b"b"),
        ..left
    };
    let unknown = Observation {
        capability: Capability::Unavailable,
        ..left
    };
    let exact = exact_size(9);
    let file = file(1, &exact);
    let folder = Side {
        item_no: 2,
        kind: ResultItemKind::Directory,
        size: Some(&exact),
    };
    for (comparison, location, expected) in [
        (
            RootComparison::new(left, other),
            Location::Incomparable(Gap::Excluded),
            Gap::DifferentStorage,
        ),
        (
            RootComparison::new(left, unknown),
            Location::Incomparable(Gap::AccessDenied),
            Gap::UnknownStorage,
        ),
        (
            RootComparison::new(left, unknown),
            Location::Incomparable(Gap::DifferentStorage),
            Gap::DifferentStorage,
        ),
    ] {
        let Some(Rows::One(row)) =
            comparison.classify(Some(file), Some(folder), location, &SizeBasis::Logical)
        else {
            return Err("expected one incomparable row");
        };
        assert_eq!(row.change, Change::Incomparable(expected));
        assert_eq!(
            (row.before, row.after, row.delta),
            (Some(file), Some(folder), None)
        );
    }
    Ok(())
}
