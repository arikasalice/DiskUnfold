use du_analysis_index::{BuildError, IngestSink, Observation};
use du_core_contracts::result::{ResultCompleteness, ResultVersionRead};
use du_core_contracts::{ByteSize, Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn value(n: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(n),
        },
    }
}
fn add(
    build: &mut du_analysis_index::BuildHandle,
    path: &[&str],
    size: Option<SizeValue>,
) -> Result<(), BuildError> {
    build.add(Observation {
        path: path.iter().map(|p| p.as_bytes().to_vec()).collect(),
        size,
    })
}
fn err(e: BuildError) -> String {
    format!("{e:?}")
}

#[test]
fn pinned_subtree_totals_preserve_basis_and_version() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    add(&mut build, &[], Some(value(0))).map_err(err)?;
    add(&mut build, &["folder"], Some(value(0))).map_err(err)?;
    add(&mut build, &["folder", "file"], Some(value(42))).map_err(err)?;
    let first = build.publish_partial(id(2)?).map_err(err)?;
    assert_eq!(
        first.space(0).ok_or("root")?.logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(42)
        }
    );
    add(&mut build, &["other"], Some(value(8))).map_err(err)?;
    let second = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let root = second.space(0).ok_or("root")?;
    assert_eq!(
        root.logical.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(50)
        }
    );
    assert_eq!(root.allocated.amount, SizeAmount::Unknown);
    assert_eq!(root.unique_physical.amount, SizeAmount::Unknown);
    assert_eq!(
        first.space(0).ok_or("root")?.logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(42)
        }
    );
    let folder = second
        .items()
        .iter()
        .find(|i| i.name == b"folder")
        .ok_or("folder")?;
    assert_eq!(
        second.space(folder.item_no).ok_or("space")?.logical.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(42)
        }
    );
    assert!(second.space(999).is_none());
    Ok(())
}

#[test]
fn missing_unknown_zero_and_cancelled_remain_distinct() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    add(&mut build, &[], Some(value(0))).map_err(err)?;
    add(&mut build, &["unknown"], None).map_err(err)?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let root = result.space(0).ok_or("root")?;
    assert_eq!(
        root.logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(0)
        }
    );
    assert_eq!(root.completeness, ResultCompleteness::Missing);
    assert_eq!(root.missing_items, 1);
    assert_eq!(
        result.space(1).ok_or("child")?.logical.amount,
        SizeAmount::Unknown
    );
    let mut cancelled = sink.open(id(1)?).map_err(err)?;
    add(&mut cancelled, &[], Some(value(0))).map_err(err)?;
    let result = cancelled
        .seal(id(3)?, ResultCompleteness::Cancelled)
        .map_err(err)?;
    assert_eq!(
        result.space(0).ok_or("root")?.completeness,
        ResultCompleteness::Cancelled
    );
    assert_eq!(
        result.space(0).ok_or("root")?.logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(0)
        }
    );
    Ok(())
}

#[test]
fn overflow_and_wrong_basis_never_become_exact() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    add(&mut build, &[], Some(value(u64::MAX))).map_err(err)?;
    add(&mut build, &["extra"], Some(value(1))).map_err(err)?;
    let mut wrong = value(42);
    wrong.basis = SizeBasis::Allocated;
    add(&mut build, &["wrong"], Some(wrong)).map_err(err)?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let root = result.space(0).ok_or("root")?;
    assert_eq!(root.logical.amount, SizeAmount::Unknown);
    assert!(root.limitations.contains(&"space.aggregation_overflow"));
    assert_eq!(root.missing_items, 1);
    Ok(())
}

#[test]
fn weakest_evidence_and_input_lower_bound_survive() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    add(&mut build, &[], Some(value(0))).map_err(err)?;
    let mut uncertain = value(9);
    uncertain.evidence = SizeEvidence::Estimated;
    uncertain.amount = SizeAmount::LowerBound {
        bytes: ByteSize::from(9),
    };
    add(&mut build, &["file"], Some(uncertain)).map_err(err)?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let root = result.space(0).ok_or("root")?;
    assert_eq!(root.logical.evidence, SizeEvidence::Estimated);
    assert_eq!(
        root.logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(9)
        }
    );
    assert_eq!(root.completeness, ResultCompleteness::Missing);
    Ok(())
}

#[test]
fn structures_distinguish_empty_directory_excluded_link_and_unknown_file()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    build.add_structure(vec![]).map_err(err)?;
    build.add_structure(vec![b"empty".to_vec()]).map_err(err)?;
    build.add_structure(vec![b"link".to_vec()]).map_err(err)?;
    let empty = build.publish_partial(id(2)?).map_err(err)?;
    assert_eq!(
        empty.space(0).ok_or("root")?.logical.amount,
        SizeAmount::Unknown
    );
    add(&mut build, &["unknown"], None).map_err(err)?;
    add(&mut build, &["known"], Some(value(7))).map_err(err)?;
    let result = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(err)?;
    for name in [b"empty".as_slice(), b"link".as_slice()] {
        let item = result
            .items()
            .iter()
            .find(|item| item.name == name)
            .ok_or("item")?;
        let space = result.space(item.item_no).ok_or("space")?;
        assert_eq!(
            space.logical.amount,
            SizeAmount::Exact {
                bytes: ByteSize::from(0)
            }
        );
        assert_eq!(space.completeness, ResultCompleteness::Complete);
    }
    let unknown = result
        .items()
        .iter()
        .find(|item| item.name == b"unknown")
        .ok_or("unknown")?;
    assert_eq!(
        result.space(unknown.item_no).ok_or("space")?.logical.amount,
        SizeAmount::Unknown
    );
    let root = result.space(0).ok_or("root")?;
    assert_eq!(
        root.logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(7)
        }
    );
    assert_eq!(root.missing_items, 1);
    assert_eq!(root.allocated.amount, SizeAmount::Unknown);
    Ok(())
}

#[test]
fn duplicate_structure_aborts_and_restores_previous_version()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut first = sink.open(id(1)?).map_err(err)?;
    first.add_structure(vec![]).map_err(err)?;
    let previous = first
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(
        previous.space(0).ok_or("root")?.logical.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(0)
        }
    );
    let mut next = sink.open(id(1)?).map_err(err)?;
    next.add_structure(vec![]).map_err(err)?;
    next.publish_partial(id(3)?).map_err(err)?;
    assert_eq!(next.add_structure(vec![]), Err(BuildError::InvalidTree));
    assert_eq!(
        sink.pin().map_err(err)?.ok_or("current")?.version_id(),
        previous.version_id()
    );
    Ok(())
}
