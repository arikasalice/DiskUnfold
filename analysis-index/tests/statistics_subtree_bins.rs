use du_analysis_index::{
    IngestSink, Observation,
    statistics_distribution::strategies::{Error, SubtreeError, subtree_bins},
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

#[test]
fn large_input_cancels_during_summary_and_row_materialization() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(10)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    const FILES: usize = 5_000;
    for i in 0..FILES {
        build
            .add_file(Observation {
                path: vec![format!("file-{i:05}").into_bytes()],
                size: Some(value(i as u64, false)),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(11)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let scope = ItemRef {
        result_version: version.version_id().clone(),
        item_no: 0,
    };
    let mut checks = 0;
    let early = subtree_bins(
        &version,
        &scope,
        &SizeBasis::Logical,
        &[0, 5000],
        64,
        &mut || {
            checks += 1;
            checks == 50
        },
    );
    assert!(matches!(
        early,
        Err(SubtreeError::Summary(
            du_analysis_index::statistics_summary::Error::Cancelled
        ))
    ));
    assert_eq!(checks, 50);
    checks = 0;
    let during_rows = subtree_bins(
        &version,
        &scope,
        &SizeBasis::Logical,
        &[0, 5000],
        FILES,
        &mut || {
            checks += 1;
            checks == FILES + 10
        },
    );
    assert!(matches!(
        during_rows,
        Err(SubtreeError::Strategy(Error::Cancelled))
    ));
    assert_eq!(checks, FILES + 10);
    Ok(())
}
fn value(n: u64, lower: bool) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: if lower {
            SizeAmount::LowerBound { bytes: n.into() }
        } else {
            SizeAmount::Exact { bytes: n.into() }
        },
    }
}

#[test]
fn pinned_f03_files_preserve_missing_lower_bounds_and_partition_results() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, size) in [
        ("c", None),
        ("a", Some(value(0, false))),
        ("b", Some(value(10, true))),
        ("d", Some(value(20, false))),
        ("e", Some(value(21, false))),
    ] {
        build
            .add_file(Observation {
                path: vec![name.as_bytes().to_vec()],
                size,
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .add_untraversed_link(vec![b"link".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let old = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"later".to_vec()],
            size: Some(value(1, false)),
        })
        .map_err(|e| format!("{e:?}"))?;
    let _new = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let scope = ItemRef {
        result_version: old.version_id().clone(),
        item_no: 0,
    };
    let first = subtree_bins(
        &old,
        &scope,
        &SizeBasis::Logical,
        &[0, 10, 20],
        1,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let other = subtree_bins(
        &old,
        &scope,
        &SizeBasis::Logical,
        &[0, 10, 20],
        50,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.computed.bins, other.computed.bins);
    assert_eq!(first.summary.aggregate, *old.space(0).ok_or("aggregate")?);
    assert_eq!(first.summary.counts.files, 5);
    assert_eq!(first.summary.counts.untraversed_links, 1);
    assert_eq!(first.computed.bins.intervals[0].count, 1);
    assert_eq!(first.computed.bins.intervals[1].count, 2);
    assert_eq!(first.computed.bins.intervals[1].known_bytes, 30);
    assert_eq!(first.computed.bins.intervals[1].lower_bound_count, 1);
    assert_eq!(first.computed.bins.outside.known_bytes, 21);
    assert_eq!(first.computed.bins.unknown.unknown_count, 1);
    let physical = subtree_bins(
        &old,
        &scope,
        &SizeBasis::UniquePhysical,
        &[0, 10],
        2,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(physical.computed.bins.unknown.unknown_count, 5);
    assert!(matches!(
        subtree_bins(&old, &scope, &SizeBasis::Logical, &[0, 10], 0, &mut || {
            false
        }),
        Err(SubtreeError::Strategy(Error::InvalidChunkSize))
    ));
    let mut checks = 0;
    assert!(
        subtree_bins(&old, &scope, &SizeBasis::Logical, &[0, 10], 1, &mut || {
            checks += 1;
            checks == 12
        })
        .is_err()
    );
    Ok(())
}
