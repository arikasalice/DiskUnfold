use du_analysis_index::{
    IngestSink, Observation,
    visualization::{
        geometry::{Rect, Scale, Weight},
        histogram::{Basis, Bin},
        histogram_projection::{self, Error, Request},
    },
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

#[test]
fn pinned_subtree_preserves_boundaries_and_independent_f03_total() -> Result<(), String> {
    let id = |n| Id::from_v7_bytes([n; 16]);
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![b"dir".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    for (path, bytes) in [
        (vec![b"a".to_vec()], 9),
        (vec![b"dir".to_vec(), b"b".to_vec()], 10),
    ] {
        build
            .add_file(Observation {
                path,
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: bytes.into(),
                    },
                }),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let scope = ItemRef {
        result_version: version.version_id().clone(),
        item_no: 0,
    };
    let bins = [
        Bin {
            lower: 0,
            upper: Some(10),
        },
        Bin {
            lower: 10,
            upper: None,
        },
    ];
    let request = || Request {
        version: &version,
        scope: &scope,
        size_basis: SizeBasis::Logical,
        bins: &bins,
        bounds: Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        histogram_basis: Basis::ItemCount,
        scale: Scale::Automatic,
    };
    let model =
        histogram_projection::project(request(), &mut || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(model.summary.result_version, id(2));
    assert_eq!(model.summary.counts.files, 2);
    assert_eq!(model.histogram.observed_items, 2);
    assert_eq!(model.histogram.bins[0].item_count, Weight::Exact(1));
    assert_eq!(model.histogram.bins[1].item_count, Weight::Exact(1));
    assert_eq!(model.histogram.bins[1].bytes, Weight::Exact(10));
    assert_eq!(
        model.summary.aggregate.logical,
        version.space(0).ok_or("missing total")?.logical
    );
    assert!(model.histogram.bins.iter().all(|bin| bin.rect.is_some()));
    assert!(matches!(
        histogram_projection::project(request(), &mut || true),
        Err(Error::Cancelled)
    ));
    let stale = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let mut wrong = request();
    wrong.scope = &stale;
    assert!(matches!(
        histogram_projection::project(wrong, &mut || false),
        Err(Error::Source(_))
    ));
    Ok(())
}

#[test]
fn unknown_file_sizes_and_partial_scope_are_not_fabricated() -> Result<(), String> {
    let id = |n| Id::from_v7_bytes([n; 16]);
    let sink = IngestSink::default();
    let mut build = sink.open(id(4)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"unknown".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(5), ResultCompleteness::Partial)
        .map_err(|e| format!("{e:?}"))?;
    let scope = ItemRef {
        result_version: id(5),
        item_no: 0,
    };
    let bins = [Bin {
        lower: 0,
        upper: None,
    }];
    let result = histogram_projection::project(
        Request {
            version: &version,
            scope: &scope,
            size_basis: SizeBasis::Logical,
            bins: &bins,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            histogram_basis: Basis::Bytes,
            scale: Scale::Automatic,
        },
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        result.summary.result_completeness,
        ResultCompleteness::Partial
    );
    assert_eq!(result.histogram.uncertain_items.len(), 1);
    assert_eq!(result.histogram.bins[0].bytes, Weight::Unknown);
    assert!(result.histogram.bins[0].rect.is_none());
    Ok(())
}

#[test]
fn source_state_is_preserved_when_scope_has_an_untraversed_link() -> Result<(), String> {
    let id = |n| Id::from_v7_bytes([n; 16]);
    for (source_state, expected) in [
        (ResultCompleteness::Complete, ResultCompleteness::Partial),
        (ResultCompleteness::Partial, ResultCompleteness::Partial),
        (ResultCompleteness::Missing, ResultCompleteness::Missing),
        (ResultCompleteness::Cancelled, ResultCompleteness::Cancelled),
    ] {
        let sink = IngestSink::default();
        let mut build = sink.open(id(7)).map_err(|e| format!("{e:?}"))?;
        build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        build
            .add_untraversed_link(vec![b"link".to_vec()])
            .map_err(|e| format!("{e:?}"))?;
        let version = build
            .seal(id(8), source_state)
            .map_err(|e| format!("{e:?}"))?;
        let scope = ItemRef {
            result_version: id(8),
            item_no: 0,
        };
        let bins = [Bin {
            lower: 0,
            upper: None,
        }];
        let model = histogram_projection::project(
            Request {
                version: &version,
                scope: &scope,
                size_basis: SizeBasis::Logical,
                bins: &bins,
                bounds: Rect {
                    left: 0,
                    top: 0,
                    right: 100,
                    bottom: 100,
                },
                histogram_basis: Basis::ItemCount,
                scale: Scale::Automatic,
            },
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
        assert_eq!(model.summary.result_completeness, source_state);
        assert_eq!(model.histogram.source_completeness, expected);
        assert_eq!(model.summary.counts.untraversed_links, 1);
    }
    Ok(())
}

#[test]
fn recorded_gap_does_not_erase_missing_or_cancelled_state() -> Result<(), String> {
    let id = |n| Id::from_v7_bytes([n; 16]);
    for requested in [ResultCompleteness::Complete, ResultCompleteness::Cancelled] {
        let sink = IngestSink::default();
        let mut build = sink.open(id(11)).map_err(|e| format!("{e:?}"))?;
        build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        build
            .record_gap_at(0, vec![], "scan.path_denied".into(), None)
            .map_err(|e| format!("{e:?}"))?;
        let version = build
            .seal(id(12), requested)
            .map_err(|e| format!("{e:?}"))?;
        let scope = ItemRef {
            result_version: id(12),
            item_no: 0,
        };
        let bins = [Bin {
            lower: 0,
            upper: None,
        }];
        let model = histogram_projection::project(
            Request {
                version: &version,
                scope: &scope,
                size_basis: SizeBasis::Logical,
                bins: &bins,
                bounds: Rect {
                    left: 0,
                    top: 0,
                    right: 100,
                    bottom: 100,
                },
                histogram_basis: Basis::ItemCount,
                scale: Scale::Automatic,
            },
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
        let expected = if requested == ResultCompleteness::Complete {
            ResultCompleteness::Missing
        } else {
            requested
        };
        assert_eq!(model.histogram.source_completeness, expected);
        assert_eq!(model.summary.result_completeness, expected);
        assert_eq!(model.summary.source_gaps.len(), 1);
    }
    Ok(())
}

#[test]
fn another_root_gap_stays_visible_without_rewriting_source_state() -> Result<(), String> {
    let id = |n| Id::from_v7_bytes([n; 16]);
    let root = |name: &[u8], n| ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(name),
        },
        volume_id: vec![n],
        file_id: vec![n],
    };
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(13), vec![root(b"/a", 1), root(b"/b", 2)])
        .map_err(|e| format!("{e:?}"))?;
    for n in 0..2 {
        build
            .add_directory_at(n, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .record_gap_at(1, vec![], "scan.path_denied".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(14), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let scope = ItemRef {
        result_version: id(14),
        item_no: 0,
    };
    let bins = [Bin {
        lower: 0,
        upper: None,
    }];
    let model = histogram_projection::project(
        Request {
            version: &version,
            scope: &scope,
            size_basis: SizeBasis::Logical,
            bins: &bins,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            histogram_basis: Basis::ItemCount,
            scale: Scale::Automatic,
        },
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(model.summary.source_gaps[0].root_index, 1);
    assert_eq!(model.summary.aggregate.inaccessible_ranges, None);
    assert_eq!(
        model.histogram.source_completeness,
        ResultCompleteness::Missing
    );
    Ok(())
}
