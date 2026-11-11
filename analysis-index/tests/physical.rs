use du_analysis_index::{
    BuildError, FileIdentity, IngestSink, Observation, SharingEvidence, SpaceObservation,
};
use du_core_contracts::result::{ResultCompleteness, ResultVersionRead};
use du_core_contracts::{ByteSize, Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn error(e: BuildError) -> String {
    format!("{e:?}")
}
fn size(basis: SizeBasis, evidence: SizeEvidence, n: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(n),
        },
    }
}
fn add(
    build: &mut du_analysis_index::BuildHandle,
    path: &[&str],
    file: u8,
    n: u64,
    sharing: SharingEvidence,
) -> Result<(), BuildError> {
    build.add_with_space(
        Observation {
            path: path.iter().map(|p| p.as_bytes().to_vec()).collect(),
            size: Some(size(SizeBasis::Logical, SizeEvidence::Measured, n)),
        },
        SpaceObservation {
            allocated: Some(size(SizeBasis::Allocated, SizeEvidence::OsReported, n)),
            identity: Some(FileIdentity {
                volume: vec![1],
                file: vec![file],
            }),
            link_count: Some(2),
            sharing,
        },
    )
}
fn exact(n: u64) -> SizeAmount {
    SizeAmount::Exact {
        bytes: ByteSize::from(n),
    }
}
fn lower(n: u64) -> SizeAmount {
    SizeAmount::LowerBound {
        bytes: ByteSize::from(n),
    }
}

#[test]
fn hardlinks_are_counted_once_per_subtree_and_versions_are_pinned()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(error)?;
    b.add_structure(vec![]).map_err(error)?;
    b.add_structure(vec![b"a".to_vec()]).map_err(error)?;
    b.add_structure(vec![b"b".to_vec()]).map_err(error)?;
    add(&mut b, &["a", "one"], 7, 8, SharingEvidence::None).map_err(error)?;
    let first = b.publish_partial(id(2)?).map_err(error)?;
    add(&mut b, &["b", "two"], 7, 8, SharingEvidence::None).map_err(error)?;
    let end = b
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(error)?;
    let root = end.space(0).ok_or("root")?;
    assert_eq!(root.logical.amount, exact(16));
    assert_eq!(root.allocated.amount, exact(8));
    assert_eq!(root.unique_physical.amount, exact(8));
    // With no shared extents, the unique total is derived from OS-reported allocation.
    assert_eq!(root.unique_physical.evidence, SizeEvidence::OsReported);
    assert_eq!(first.space(0).ok_or("root")?.allocated.amount, lower(8));
    // Each subtree retains the shared file, even though the scope counts it once.
    for name in [b"a", b"b"] {
        let dir = end
            .items()
            .iter()
            .find(|i| i.name == name)
            .ok_or("directory")?;
        let aggregate = end.space(dir.item_no).ok_or("space")?;
        assert_eq!(aggregate.allocated.amount, exact(8));
        assert_eq!(aggregate.unique_physical.amount, exact(8));
    }
    Ok(())
}

#[test]
fn empty_folder_is_zero_but_identity_free_single_links_cannot_be_deduplicated()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(error)?;
    b.add_structure(vec![]).map_err(error)?;
    b.add_structure(vec![b"empty".to_vec()]).map_err(error)?;
    for name in [b"first".as_slice(), b"alias".as_slice()] {
        b.add_with_space(
            Observation {
                path: vec![name.to_vec()],
                size: None,
            },
            SpaceObservation {
                allocated: Some(size(SizeBasis::Allocated, SizeEvidence::OsReported, 9)),
                identity: None,
                link_count: Some(1),
                sharing: SharingEvidence::None,
            },
        )
        .map_err(error)?;
    }
    let result = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    assert_eq!(
        result.space(0).ok_or("root")?.allocated.amount,
        SizeAmount::Unknown
    );
    let empty = result
        .items()
        .iter()
        .find(|item| item.name == b"empty")
        .ok_or("empty")?;
    assert_eq!(
        result.space(empty.item_no).ok_or("space")?.allocated.amount,
        exact(0)
    );
    assert_eq!(
        result
            .space(empty.item_no)
            .ok_or("space")?
            .unique_physical
            .amount,
        exact(0)
    );
    Ok(())
}

#[test]
fn legacy_input_distinguishes_empty_from_missing_without_physical_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(error)?;
    b.add_structure(vec![]).map_err(error)?;
    b.add_structure(vec![b"empty".to_vec()]).map_err(error)?;
    b.add(Observation {
        path: vec![b"missing".to_vec()],
        size: None,
    })
    .map_err(error)?;
    let result = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    assert_eq!(
        result.space(0).ok_or("root")?.allocated.amount,
        SizeAmount::Unknown
    );
    let empty = result
        .items()
        .iter()
        .find(|item| item.name == b"empty")
        .ok_or("empty")?;
    assert_eq!(
        result.space(empty.item_no).ok_or("space")?.allocated.amount,
        exact(0)
    );
    Ok(())
}

#[test]
fn missing_identity_sharing_or_conflicting_observations_never_become_exact()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(error)?;
    b.add_structure(vec![]).map_err(error)?;
    add(&mut b, &["a"], 1, 10, SharingEvidence::None).map_err(error)?;
    add(&mut b, &["b"], 1, 11, SharingEvidence::None).map_err(error)?;
    add(&mut b, &["c"], 2, 5, SharingEvidence::Unknown).map_err(error)?;
    b.add_with_space(
        Observation {
            path: vec![b"d".to_vec()],
            size: None,
        },
        SpaceObservation {
            allocated: Some(size(SizeBasis::Allocated, SizeEvidence::OsReported, 20)),
            identity: None,
            link_count: None,
            sharing: SharingEvidence::Unknown,
        },
    )
    .map_err(error)?;
    let result = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    let root = result.space(0).ok_or("root")?;
    assert_eq!(root.allocated.amount, lower(5));
    assert_eq!(root.unique_physical.amount, SizeAmount::Unknown);
    assert_eq!(root.allocated_completeness, ResultCompleteness::Missing);
    assert!(
        root.limitations
            .contains(&"space.hardlink_identity_unknown")
    );
    Ok(())
}

#[test]
fn overflow_and_untrusted_evidence_remain_unknown() -> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(error)?;
    b.add_structure(vec![]).map_err(error)?;
    add(&mut b, &["a"], 1, u64::MAX, SharingEvidence::None).map_err(error)?;
    add(&mut b, &["b"], 2, 1, SharingEvidence::None).map_err(error)?;
    let result = b
        .seal(id(2)?, ResultCompleteness::Cancelled)
        .map_err(error)?;
    let root = result.space(0).ok_or("root")?;
    assert_eq!(root.allocated.amount, SizeAmount::Unknown);
    assert_eq!(root.unique_physical.amount, SizeAmount::Unknown);
    assert_eq!(root.allocated_completeness, ResultCompleteness::Cancelled);
    assert!(root.limitations.contains(&"space.aggregation_overflow"));
    let mut b = sink.open(id(1)?).map_err(error)?;
    b.add_structure(vec![]).map_err(error)?;
    b.add_with_space(
        Observation {
            path: vec![b"estimate".to_vec()],
            size: None,
        },
        SpaceObservation {
            allocated: Some(size(SizeBasis::Allocated, SizeEvidence::Estimated, 20)),
            identity: None,
            link_count: Some(1),
            sharing: SharingEvidence::None,
        },
    )
    .map_err(error)?;
    let result = b
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(error)?;
    assert_eq!(
        result.space(0).ok_or("root")?.allocated.amount,
        SizeAmount::Unknown
    );
    Ok(())
}
