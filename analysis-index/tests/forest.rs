use du_analysis_index::{
    FileIdentity, IngestSink, Observation, QueryError, SharingEvidence, SpaceObservation, browse,
};
use du_core_contracts::{
    ByteSize, Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn bytes(path: &str) -> Vec<u8> {
    path.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
fn root(path: &str, volume: u8) -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&bytes(path)),
        },
        volume_id: vec![volume],
        file_id: vec![9],
    }
}
fn amount(value: u64, basis: SizeBasis, evidence: SizeEvidence) -> SizeValue {
    SizeValue {
        basis,
        evidence,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(value),
        },
    }
}
fn path() -> Vec<Vec<u8>> {
    vec![bytes("same.txt")]
}
fn physical(volume: u8) -> SpaceObservation {
    SpaceObservation {
        allocated: Some(amount(10, SizeBasis::Allocated, SizeEvidence::OsReported)),
        identity: Some(FileIdentity {
            volume: vec![volume],
            file: vec![1],
        }),
        link_count: Some(2),
        sharing: SharingEvidence::None,
    }
}
fn add_file(
    build: &mut du_analysis_index::BuildHandle,
    index: u32,
    volume: u8,
) -> Result<(), String> {
    build
        .add_with_space_at(
            index,
            Observation {
                path: path(),
                size: Some(amount(10, SizeBasis::Logical, SizeEvidence::Measured)),
            },
            physical(volume),
        )
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn forest_roots_keep_identity_and_scope_deduplicates_across_volumes() -> Result<(), String> {
    let sink = IngestSink::default();
    let roots = vec![
        root("C:\\First\\Same", 1),
        root("C:\\Second\\Same", 1),
        root("D:\\Same", 2),
    ];
    let mut build = sink
        .open_forest(id(1)?, roots.clone())
        .map_err(|e| format!("{e:?}"))?;
    for index in 0..3 {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    add_file(&mut build, 0, 1)?;
    add_file(&mut build, 1, 1)?;
    add_file(&mut build, 2, 2)?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.observed_root().is_none());
    assert_eq!(result.observed_roots(), roots);
    assert_eq!(result.scope_roots().len(), 3);
    assert!(
        result
            .scope_roots()
            .iter()
            .all(|item| item.parent.is_none())
    );
    assert!(matches!(
        browse::open(&result, None),
        Err(QueryError::PathUnavailable)
    ));
    match browse::open_scope(&result).map_err(|e| format!("{e:?}"))? {
        browse::ScopeView::Forest { roots, total } => {
            assert_eq!(roots.len(), 3);
            assert_eq!(total, result.scope_total());
        }
        browse::ScopeView::Single(_) => return Err("forest exposed as a single root".into()),
    }
    assert_eq!(
        result.scope_total().logical.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(30)
        }
    );
    assert_eq!(
        result.scope_total().allocated.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(20)
        }
    );
    assert_eq!(
        result.scope_total().unique_physical.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(20)
        }
    );
    for (index, prefix) in ["C:\\First\\Same", "C:\\Second\\Same", "D:\\Same"]
        .into_iter()
        .enumerate()
    {
        let item = ItemRef {
            result_version: id(2)?,
            item_no: (index + 3) as u32,
        };
        let native = result.native_path(&item).map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            result
                .observed_root_for(&item)
                .map_err(|e| format!("{e:?}"))?
                .0,
            index as u32
        );
        assert_eq!(
            native.raw.bytes().map_err(|e| e.to_string())?,
            bytes(&format!("{prefix}\\same.txt"))
        );
    }
    Ok(())
}

#[test]
fn gaps_are_pinned_per_root_and_never_become_zero_or_complete() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root("C:\\A", 1), root("D:\\B", 2)])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(1, vec![])
        .map_err(|e| format!("{e:?}"))?;
    add_file(&mut build, 0, 1)?;
    build
        .record_gap_at(1, vec![], "scan.path_denied".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    let partial = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(partial.completeness(), ResultCompleteness::Partial);
    let result = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    assert_eq!(result.scope_gaps().len(), 1);
    assert_eq!(result.scope_total().inaccessible_ranges, Some(1));
    assert_eq!(
        result.scope_total().logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(10)
        }
    );
    assert_eq!(
        result.space(1).ok_or("root")?.logical.amount,
        SizeAmount::Unknown
    );
    assert_eq!(
        result.space(0).ok_or("root")?.logical.amount,
        SizeAmount::Exact {
            bytes: ByteSize::from(10)
        }
    );
    assert_eq!(partial.scope_gaps(), result.scope_gaps());
    Ok(())
}

#[test]
fn one_root_forest_preserves_existing_root_and_default_browse() -> Result<(), String> {
    let sink = IngestSink::default();
    let evidence = root("C:\\Only", 1);
    let mut build = sink
        .open_forest(id(1)?, vec![evidence.clone()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.observed_root(), Some(&evidence));
    assert_eq!(
        browse::open(&result, None)
            .map_err(|e| format!("{e:?}"))?
            .location
            .item_no,
        0
    );
    assert_eq!(result.scope_total(), result.space(0).ok_or("root")?);
    assert!(std::ptr::eq(
        result.scope_total(),
        result.space(0).ok_or("root")?
    ));
    Ok(())
}

#[test]
fn invalid_forest_gap_and_unknown_insertion_preserve_prior_publication() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut baseline = sink
        .open_observed(id(1)?, root("C:\\Before", 1))
        .map_err(|e| format!("{e:?}"))?;
    baseline
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    let original = baseline
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let roots = vec![root("C:\\A", 1), root("D:\\B", 2)];
    let mut build = sink
        .open_forest(id(3)?, roots.clone())
        .map_err(|e| format!("{e:?}"))?;
    for index in 0..2 {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .add_at(
            0,
            Observation {
                path: path(),
                size: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    let partial = build
        .publish_partial(id(4)?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        build
            .record_gap_at(2, vec![], "scan.path_denied".into(), None)
            .is_err()
    );
    build.abort();
    assert_eq!(
        sink.pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing baseline")?
            .version_id(),
        original.version_id()
    );
    assert_eq!(partial.completeness(), ResultCompleteness::Partial);

    let mut build = sink
        .open_forest(id(5)?, roots)
        .map_err(|e| format!("{e:?}"))?;
    for index in 0..2 {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .add_at(
            0,
            Observation {
                path: path(),
                size: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .publish_partial(id(6)?)
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        build
            .add_at(
                0,
                Observation {
                    path: path(),
                    size: None
                }
            )
            .is_err()
    );
    assert_eq!(
        sink.pin()
            .map_err(|e| format!("{e:?}"))?
            .ok_or("missing baseline")?
            .version_id(),
        original.version_id()
    );
    Ok(())
}

#[test]
fn one_root_gap_is_included_in_the_reused_scope_aggregate() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root("C:\\A", 1)])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    add_file(&mut build, 0, 1)?;
    build
        .record_gap_at(0, vec![], "scan.path_denied".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(std::ptr::eq(
        result.scope_total(),
        result.space(0).ok_or("root")?
    ));
    assert_eq!(
        result.scope_total().logical.amount,
        SizeAmount::LowerBound {
            bytes: ByteSize::from(10)
        }
    );
    assert_eq!(result.scope_total().inaccessible_ranges, Some(1));
    assert_eq!(
        result.scope_total().completeness,
        ResultCompleteness::Missing
    );
    Ok(())
}

#[test]
fn unknown_observation_and_cancelled_root_gap_remain_incomplete() -> Result<(), String> {
    use du_core_contracts::result::ResultItemKind;
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root("C:\\A", 1), root("D:\\B", 2)])
        .map_err(|e| format!("{e:?}"))?;
    for index in 0..2 {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .add_at(
            0,
            Observation {
                path: path(),
                size: None,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .record_gap_at(1, vec![], "scan.path_denied".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(2)?, ResultCompleteness::Cancelled)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.items()[2].kind, ResultItemKind::Unknown);
    assert_eq!(result.scope_total().logical.amount, SizeAmount::Unknown);
    assert_eq!(
        result.scope_total().completeness,
        ResultCompleteness::Cancelled
    );
    assert_eq!(result.scope_total().inaccessible_ranges, Some(1));
    Ok(())
}

#[test]
fn missing_root_cannot_be_published_as_an_empty_success() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root("C:\\A", 1), root("D:\\B", 2)])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        build.publish_partial(id(2)?),
        Err(du_analysis_index::BuildError::InvalidTree)
    ));
    assert!(sink.pin().map_err(|e| format!("{e:?}"))?.is_none());
    Ok(())
}

#[test]
fn volume_reports_are_frozen_independently_of_aggregates() -> Result<(), String> {
    use du_analysis_index::{VolumeReport, VolumeReportPoint};
    use du_core_contracts::scan_observation::VolumeSpace;
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root("C:\\A", 1)])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let point = VolumeReportPoint {
        observed_at: None,
        value: Ok(VolumeSpace {
            total_bytes: 100,
            used_bytes: 40,
            free_bytes: 60,
        }),
    };
    let mut report = VolumeReport {
        volume_id: vec![1],
        start: Some(point.clone()),
        end: None,
    };
    build
        .set_volume_reports(vec![report.clone()])
        .map_err(|e| format!("{e:?}"))?;
    let partial = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    report.end = Some(point);
    build
        .set_volume_reports(vec![report.clone()])
        .map_err(|e| format!("{e:?}"))?;
    let finished = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(partial.volume_reports()[0].end.is_none());
    assert_eq!(finished.volume_reports(), &[report]);
    let mut control = sink
        .open_forest(id(4)?, vec![root("C:\\A", 1)])
        .map_err(|e| format!("{e:?}"))?;
    control
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let control = control
        .seal(id(5)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        control.scope_total().logical.amount,
        finished.scope_total().logical.amount
    );
    assert_eq!(
        control.scope_total().allocated.amount,
        finished.scope_total().allocated.amount
    );
    Ok(())
}

#[test]
fn observation_range_merges_forest_roots_and_respects_local_gaps() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(90)?, vec![root("C:\\First", 1), root("D:\\Second", 2)])
        .map_err(|e| format!("{e:?}"))?;
    let early = du_core_contracts::Timestamp::from_unix_millis(1).ok_or("time")?;
    let late = du_core_contracts::Timestamp::from_unix_millis(2).ok_or("time")?;
    for index in 0..2 {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .set_item_observed_at(0, &[], early.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_item_observed_at(1, &[], late.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .record_gap_at(1, vec![], "scan.path_denied".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(91)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(result.observation_range(0).ok_or("root")?.complete);
    assert!(!result.observation_range(1).ok_or("root")?.complete);
    let scope = result.scope_observation_range();
    assert_eq!(
        (scope.min, scope.max, scope.complete),
        (Some(&early), Some(&late), false)
    );
    Ok(())
}
