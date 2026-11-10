use super::*;
use crate::{
    comparison::{Change, folder::Child},
    records::item::{Item, Kind, Size},
    storage_match::{Capability, Observation},
};
use du_core_contracts::SizeEvidence;

fn value(basis: SizeBasis, amount: SizeAmount) -> SizeValue {
    SizeValue {
        basis,
        amount,
        evidence: SizeEvidence::Measured,
    }
}
fn aggregate(n: u64) -> SpaceAggregate {
    SpaceAggregate {
        logical: value(SizeBasis::Logical, SizeAmount::Exact { bytes: n.into() }),
        allocated: value(SizeBasis::Allocated, SizeAmount::Exact { bytes: n.into() }),
        unique_physical: value(
            SizeBasis::UniquePhysical,
            SizeAmount::Exact { bytes: n.into() },
        ),
        completeness: ResultCompleteness::Complete,
        allocated_completeness: ResultCompleteness::Complete,
        unique_physical_completeness: ResultCompleteness::Complete,
        missing_items: 0,
        untraversed_links: 0,
        inaccessible_ranges: Some(0),
        limitations: Default::default(),
    }
}
struct Source {
    before: SpaceAggregate,
    after: SpaceAggregate,
    calls: Vec<Side>,
    fail: bool,
}
impl SetTotals for Source {
    type Error = &'static str;
    fn total(
        &mut self,
        side: Side,
        _: &SizeBasis,
        _: &mut dyn FnMut() -> bool,
    ) -> Result<SpaceAggregate, Self::Error> {
        self.calls.push(side);
        if self.fail && side == Side::Later {
            return Err("source");
        }
        Ok(match side {
            Side::Earlier => self.before.clone(),
            Side::Later => self.after.clone(),
        })
    }
}
fn root() -> RootComparison {
    let observation = Observation {
        capability: Capability::Supported,
        kind: Some(b"id"),
        value: Some(b"v"),
    };
    RootComparison::new(observation, observation)
}
fn item() -> Item<'static> {
    Item {
        number: 1,
        parent: 0,
        kind: Kind::File,
        flags: 0,
        name: b"x",
        sizes: [Size::Exact(10), Size::Unknown, Size::Unknown],
        modified_ms: None,
        identity: &[],
        link_count: 0,
    }
}

#[test]
fn tree_assembly_uses_set_totals_and_preserves_wide_delta() -> Result<(), Error<&'static str>> {
    let file = item();
    let before = tree::Tree::new(
        &[Child {
            item: &file,
            key: b"x",
        }],
        || false,
    )
    .map_err(|_| Error::InvalidTree)?;
    let after = tree::Tree::new(&[], || false).map_err(|_| Error::InvalidTree)?;
    let ranges = range::Ranges::new(&[], || false).map_err(|_| Error::Cancelled)?;
    let mut source = Source {
        before: aggregate(u64::MAX),
        after: aggregate(0),
        calls: vec![],
        fail: false,
    };
    let result = compare(
        Inputs {
            root: &root(),
            earlier: &before,
            later: &after,
            basis: &SizeBasis::Logical,
            ranges: &ranges,
        },
        &mut source,
        || false,
    )?;
    assert_eq!(result.rows[0].change, Change::Removed);
    assert_eq!(
        result.totals.and_then(|totals| totals.delta),
        Some(-i128::from(u64::MAX))
    );
    assert_eq!(source.calls, [Side::Earlier, Side::Later]);
    Ok(())
}

#[test]
fn unknown_partial_and_other_basis_are_not_exact() {
    let mut a = aggregate(7);
    let original = a.clone();
    assert_eq!(exact(&a, &SizeBasis::Logical), Some(7));
    a.logical.amount = SizeAmount::LowerBound { bytes: 7.into() };
    assert_eq!(exact(&a, &SizeBasis::Logical), None);
    a.logical.amount = SizeAmount::Unknown;
    assert_eq!(exact(&a, &SizeBasis::Logical), None);
    a = original;
    a.completeness = ResultCompleteness::Partial;
    assert_eq!(exact(&a, &SizeBasis::Logical), None);
    assert_eq!(exact(&a, &SizeBasis::Allocated), Some(7));
    a.allocated.basis = SizeBasis::Logical;
    assert_eq!(exact(&a, &SizeBasis::Allocated), None);
    a.unique_physical.evidence = SizeEvidence::Unknown;
    assert_eq!(exact(&a, &SizeBasis::UniquePhysical), None);
}

#[test]
fn incompatible_roots_do_not_request_a_common_set() -> Result<(), Error<&'static str>> {
    let known = Observation {
        capability: Capability::Supported,
        kind: Some(b"id"),
        value: Some(b"earlier"),
    };
    let different = Observation {
        value: Some(b"other"),
        ..known
    };
    let unavailable = Observation {
        capability: Capability::Unavailable,
        ..known
    };
    let empty = tree::Tree::new(&[], || false).map_err(|_| Error::InvalidTree)?;
    let ranges = range::Ranges::new(&[], || false).map_err(|_| Error::Cancelled)?;
    let mut source = Source {
        before: aggregate(1),
        after: aggregate(2),
        calls: vec![],
        fail: true,
    };
    for later in [different, unavailable] {
        let result = compare(
            Inputs {
                root: &RootComparison::new(known, later),
                earlier: &empty,
                later: &empty,
                basis: &SizeBasis::Logical,
                ranges: &ranges,
            },
            &mut source,
            || false,
        )?;
        assert!(result.totals.is_none());
    }
    assert!(source.calls.is_empty());
    Ok(())
}

#[test]
fn errors_and_cancellation_do_not_return_partial_comparison() -> Result<(), tree::Error> {
    let empty = tree::Tree::new(&[], || false)?;
    let ranges = range::Ranges::new(&[], || false)?;
    let mut source = Source {
        before: aggregate(1),
        after: aggregate(2),
        calls: vec![],
        fail: true,
    };
    let root = root();
    let inputs = || Inputs {
        root: &root,
        earlier: &empty,
        later: &empty,
        basis: &SizeBasis::Logical,
        ranges: &ranges,
    };
    assert!(matches!(
        compare(inputs(), &mut source, || false),
        Err(Error::Source("source"))
    ));
    source.calls.clear();
    assert!(matches!(
        compare(inputs(), &mut source, || true),
        Err(Error::Cancelled)
    ));
    assert!(source.calls.is_empty());
    Ok(())
}

#[test]
fn cancellation_during_first_total_stops_the_second() -> Result<(), tree::Error> {
    use std::cell::Cell;
    struct Cancelling<'a> {
        flag: &'a Cell<bool>,
        calls: u32,
    }
    impl SetTotals for Cancelling<'_> {
        type Error = &'static str;
        fn total(
            &mut self,
            _: Side,
            _: &SizeBasis,
            _: &mut dyn FnMut() -> bool,
        ) -> Result<SpaceAggregate, Self::Error> {
            self.calls += 1;
            self.flag.set(true);
            Err("cancelled by source")
        }
    }
    let flag = Cell::new(false);
    let empty = tree::Tree::new(&[], || false)?;
    let ranges = range::Ranges::new(&[], || false)?;
    let mut source = Cancelling {
        flag: &flag,
        calls: 0,
    };
    let result = compare(
        Inputs {
            root: &root(),
            earlier: &empty,
            later: &empty,
            basis: &SizeBasis::Logical,
            ranges: &ranges,
        },
        &mut source,
        || flag.get(),
    );
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(source.calls, 1);
    Ok(())
}
