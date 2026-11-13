use du_analysis_index::{
    BuildError, FileIdentity, IngestSink, Observation, QueryError, SharingEvidence,
    SpaceObservation,
};
use du_core_contracts::result::{ResultCompleteness, ResultItemKind, ResultVersionRead};
use du_core_contracts::{ByteSize, Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn error(e: BuildError) -> String {
    format!("{e:?}")
}
fn value(basis: SizeBasis, n: u64) -> SizeValue {
    SizeValue {
        basis,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(n),
        },
    }
}
fn reference(version: Id, item_no: u32) -> ItemRef {
    ItemRef {
        result_version: version,
        item_no,
    }
}

#[test]
fn directories_files_links_unknowns_and_item_evidence_are_distinct()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(error)?;
    b.add_directory(vec![]).map_err(error)?;
    b.add_directory(vec![b"dir".to_vec()]).map_err(error)?;
    b.add_structure(vec![b"legacy".to_vec()]).map_err(error)?;
    b.add_untraversed_link(vec![b"link".to_vec()])
        .map_err(error)?;
    b.add(Observation {
        path: vec![b"unknown".to_vec()],
        size: None,
    })
    .map_err(error)?;
    let physical = SpaceObservation {
        allocated: Some(value(SizeBasis::Allocated, 8)),
        identity: Some(FileIdentity {
            volume: vec![1],
            file: vec![2],
        }),
        link_count: Some(1),
        sharing: SharingEvidence::None,
    };
    b.add_with_space(
        Observation {
            path: vec![b"dir".to_vec(), b"file".to_vec()],
            size: Some(value(SizeBasis::Logical, 12)),
        },
        physical.clone(),
    )
    .map_err(error)?;
    let previous = b.publish_partial(id(4)?).map_err(error)?;
    let version = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    let lookup = |name: &[u8]| {
        version
            .items()
            .iter()
            .find(|item| item.name == name)
            .map(|item| item.item_no)
            .ok_or("item not found")
    };
    let file = lookup(b"file")?;
    let detail = version
        .detail(&reference(id(2)?, file))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(detail.item.kind, ResultItemKind::File);
    assert_eq!(detail.item.size, Some(value(SizeBasis::Logical, 12)));
    assert_eq!(detail.physical, Some(&physical));
    let previous_file = previous
        .items()
        .iter()
        .find(|item| item.name == b"file")
        .ok_or("previous file")?;
    assert_eq!(
        previous
            .detail(&reference(id(4)?, previous_file.item_no))
            .map_err(|e| format!("{e:?}"))?
            .physical,
        Some(&physical)
    );
    assert_eq!(
        version
            .detail(&reference(id(2)?, lookup(b"link")?))
            .map_err(|e| format!("{e:?}"))?
            .item
            .kind,
        ResultItemKind::UntraversedLink
    );
    assert_eq!(
        version
            .detail(&reference(id(2)?, lookup(b"legacy")?))
            .map_err(|e| format!("{e:?}"))?
            .item
            .kind,
        ResultItemKind::UnknownStructure
    );
    assert_eq!(
        version
            .detail(&reference(id(2)?, lookup(b"unknown")?))
            .map_err(|e| format!("{e:?}"))?
            .item
            .kind,
        ResultItemKind::Unknown
    );
    let folder = version
        .folder(&id(2)?, &[b"dir".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        folder.logical.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(12)
        }
    );
    assert_eq!(
        version.folder(&id(2)?, &[b"dir".to_vec(), b"file".to_vec()]),
        Err(QueryError::NotDirectory)
    );
    assert_eq!(
        version.folder(&id(2)?, &[b"link".to_vec()]),
        Err(QueryError::NotDirectory)
    );
    assert_eq!(
        version.folder(&id(2)?, &[b"legacy".to_vec()]),
        Err(QueryError::KindUnknown)
    );
    assert_eq!(
        version.folder(&id(2)?, &[b"absent".to_vec()]),
        Err(QueryError::NotFound)
    );
    assert_eq!(
        version.folder(&id(2)?, &[vec![]]),
        Err(QueryError::InvalidPath)
    );
    assert_eq!(
        version.space(0).ok_or("root")?.completeness,
        ResultCompleteness::Missing
    );
    assert_eq!(version.space(0).ok_or("root")?.missing_items, 1);
    assert_eq!(version.space(0).ok_or("root")?.untraversed_links, 1);
    assert_eq!(version.space(0).ok_or("root")?.inaccessible_ranges, None);
    Ok(())
}

#[test]
fn admitted_current_version_and_open_old_snapshot_have_separate_rules()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    assert!(matches!(
        sink.pin_current(&id(2)?),
        Err(QueryError::NotFound)
    ));
    let mut b = sink.open(id(1)?).map_err(error)?;
    b.add_directory(vec![]).map_err(error)?;
    let old = b.publish_partial(id(2)?).map_err(error)?;
    b.add_directory(vec![b"new".to_vec()]).map_err(error)?;
    let next = b
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(error)?;
    assert!(matches!(
        sink.pin_current(&id(2)?),
        Err(QueryError::StaleVersion)
    ));
    assert!(sink.pin_current(&id(3)?).is_ok());
    assert!(old.folder(&id(2)?, &[]).is_ok());
    assert_eq!(
        old.folder(&id(2)?, &[b"new".to_vec()]),
        Err(QueryError::NotFound)
    );
    assert_eq!(old.folder(&id(3)?, &[]), Err(QueryError::StaleVersion));
    assert!(matches!(
        next.detail(&reference(id(2)?, 0)),
        Err(QueryError::StaleVersion)
    ));
    assert!(matches!(
        next.detail(&reference(id(3)?, 999)),
        Err(QueryError::NotFound)
    ));
    Ok(())
}

#[test]
fn untraversed_counts_propagate_without_double_counting_and_snapshots_remain_fixed()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(error)?;
    b.add_directory(vec![]).map_err(error)?;
    b.add_directory(vec![b"dir".to_vec()]).map_err(error)?;
    b.add_untraversed_link(vec![b"dir".to_vec(), b"one".to_vec()])
        .map_err(error)?;
    let partial = b.publish_partial(id(2)?).map_err(error)?;
    let before = partial
        .folder(&id(2)?, &[b"dir".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(before.untraversed_links, 1);
    assert_eq!(before.missing_items, 0);
    assert_eq!(before.logical.amount, SizeAmount::Unknown);
    b.add_untraversed_link(vec![b"dir".to_vec(), b"two".to_vec()])
        .map_err(error)?;
    b.add(Observation {
        path: vec![b"dir".to_vec(), b"file".to_vec()],
        size: Some(value(SizeBasis::Logical, 7)),
    })
    .map_err(error)?;
    let complete = b
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(error)?;
    let dir = complete
        .folder(&id(3)?, &[b"dir".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(dir.untraversed_links, 2);
    assert_eq!(dir.missing_items, 0);
    assert_eq!(dir.inaccessible_ranges, None);
    assert_eq!(
        dir.logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(7)
        }
    );
    assert_eq!(dir.completeness, ResultCompleteness::Missing);
    assert_eq!(complete.space(0).ok_or("root")?.untraversed_links, 2);
    assert_eq!(partial.space(0).ok_or("root")?.untraversed_links, 1);
    Ok(())
}
