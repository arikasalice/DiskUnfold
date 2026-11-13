use du_analysis_index::{
    IngestSink, Observation,
    statistics_summary::{self, Error},
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn size(n: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact { bytes: n.into() },
    }
}
fn root(version: &du_analysis_index::ResultVersion) -> ItemRef {
    ItemRef {
        result_version: version.version_id().clone(),
        item_no: 0,
    }
}

#[test]
fn complete_subtree_copies_f03_evidence_and_does_not_sum_directories() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![b"dir".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    for (path, bytes) in [
        (vec![b"dir".to_vec(), b"a".to_vec()], 10),
        (vec![b"zero".to_vec()], 0),
    ] {
        build
            .add_file(Observation {
                path,
                size: Some(size(bytes)),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let summary = statistics_summary::summarize(&version, &root(&version), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!((summary.counts.files, summary.counts.directories), (2, 1));
    assert_eq!(summary.aggregate, *version.space(0).ok_or("no aggregate")?);
    assert_eq!(
        summary.aggregate.logical.amount,
        SizeAmount::Exact {
            bytes: 10_u64.into()
        }
    );
    assert_eq!(summary.excluded_count, None);
    Ok(())
}

#[test]
fn empty_zero_unknown_partial_and_pinned_version_are_distinct() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let empty = b.publish_partial(id(4)?).map_err(|e| format!("{e:?}"))?;
    let empty_summary = statistics_summary::summarize(&empty, &root(&empty), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(empty_summary.counts.files, 0);
    assert_eq!(
        empty_summary.result_completeness,
        ResultCompleteness::Partial
    );
    assert_eq!(empty_summary.aggregate.logical.amount, SizeAmount::Unknown);
    b.add_file(Observation {
        path: vec![b"unknown".to_vec()],
        size: None,
    })
    .map_err(|e| format!("{e:?}"))?;
    b.add_file(Observation {
        path: vec![b"zero".to_vec()],
        size: Some(size(0)),
    })
    .map_err(|e| format!("{e:?}"))?;
    let complete = b
        .seal(id(5)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let summary = statistics_summary::summarize(&complete, &root(&complete), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(summary.counts.files, 2);
    assert_eq!(summary.aggregate.missing_items, 1);
    assert_eq!(
        summary.aggregate.logical.amount,
        SizeAmount::LowerBound {
            bytes: 0_u64.into()
        }
    );
    assert_eq!(
        statistics_summary::summarize(&complete, &root(&empty), &mut || false).err(),
        Some(Error::VersionMismatch)
    );
    assert_eq!(
        statistics_summary::summarize(&empty, &root(&empty), &mut || false)
            .map_err(|e| format!("{e:?}"))?,
        empty_summary
    );
    assert_eq!(
        statistics_summary::summarize(&complete, &root(&complete), &mut || true).err(),
        Some(Error::Cancelled)
    );
    Ok(())
}

#[test]
fn complete_empty_scope_is_exact_zero() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(6)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let v = b
        .seal(id(7)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let summary = statistics_summary::summarize(&v, &root(&v), &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        summary.aggregate.logical.amount,
        SizeAmount::Exact {
            bytes: 0_u64.into()
        }
    );
    assert_eq!(summary.counts.files, 0);
    Ok(())
}
