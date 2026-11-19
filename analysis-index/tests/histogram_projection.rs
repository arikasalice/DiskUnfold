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
    for (bin, name) in model
        .histogram
        .bins
        .iter()
        .zip([b"a".as_slice(), b"b".as_slice()])
    {
        let item_no = *bin.members.first().ok_or("missing member")?;
        let item = version
            .items()
            .get(item_no as usize)
            .ok_or("invalid member")?;
        assert_eq!(item.name, name);
        assert_eq!(bin.members.len(), 1);
    }
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
fn an_untraversed_link_does_not_lower_the_source_state() -> Result<(), String> {
    let id = |n| Id::from_v7_bytes([n; 16]);
    for (source_state, expected) in [
        // F03 FR5.HD1: a link target that was not traversed is not an omission.
        (ResultCompleteness::Complete, ResultCompleteness::Complete),
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

/// Builds `/` with a 10-byte file and `links/` holding one link of the given own size.
fn linked_scope(
    link_size: Option<u64>,
) -> Result<std::sync::Arc<du_analysis_index::ResultVersion>, String> {
    let id = |n| Id::from_v7_bytes([n; 16]);
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let sized = |bytes: u64| SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    };
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(20), vec![root])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![b"links".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_file_at(
            0,
            Observation {
                path: vec![b"a".to_vec()],
                size: Some(sized(10)),
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_observed_untraversed_link_at(
            0,
            Observation {
                path: vec![b"links".to_vec(), b"up".to_vec()],
                size: link_size.map(sized),
            },
            None,
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(21), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))
}

fn histogram_of(
    version: &du_analysis_index::ResultVersion,
    item_no: u32,
) -> Result<histogram_projection::Model, String> {
    let scope = ItemRef {
        result_version: version.version_id().clone(),
        item_no,
    };
    let bins = [Bin {
        lower: 0,
        upper: None,
    }];
    histogram_projection::project(
        Request {
            version,
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
    .map_err(|e| format!("{e:?}"))
}

/// The F03 range total as a histogram weight.
fn f03_total(model: &histogram_projection::Model) -> Weight {
    match &model.summary.aggregate.logical.amount {
        SizeAmount::Exact { bytes } => Weight::Exact(bytes.get()),
        SizeAmount::LowerBound { bytes } => Weight::LowerBound(bytes.get()),
        SizeAmount::Unknown => Weight::Unknown,
    }
}

#[test]
fn links_are_binned_by_their_own_size_and_their_targets_do_not_lower_completeness()
-> Result<(), String> {
    // A link with a known own size is binned like a file: bins and total equal the F03 total.
    let version = linked_scope(Some(3))?;
    let link = version
        .items()
        .iter()
        .find(|item| item.name == b"up")
        .ok_or("missing link")?
        .item_no;
    let model = histogram_of(&version, 0)?;
    assert_eq!(
        model.histogram.source_completeness,
        ResultCompleteness::Complete
    );
    assert_eq!(model.histogram.bins[0].bytes, Weight::Exact(13));
    assert!(model.histogram.bins[0].members.contains(&link));
    assert!(model.histogram.uncertain_items.is_empty());
    assert_eq!(model.histogram.total_bytes, f03_total(&model));
    assert_eq!(f03_total(&model), Weight::Exact(13));

    // A folder that holds only that link.
    let links = version.items()[link as usize]
        .parent
        .ok_or("missing folder")?;
    let only = histogram_of(&version, links)?;
    assert_eq!(
        only.histogram.source_completeness,
        ResultCompleteness::Complete
    );
    assert_eq!(only.histogram.bins[0].bytes, Weight::Exact(3));
    assert_eq!(only.histogram.total_bytes, f03_total(&only));
    assert_eq!(f03_total(&only), Weight::Exact(3));

    // A link whose own size is unknown stays out of the bins as an uncertain item, and the
    // known part matches the F03 lower bound; the model is still not lowered to Partial.
    let version = linked_scope(None)?;
    let model = histogram_of(&version, 0)?;
    assert_eq!(
        model.histogram.source_completeness,
        ResultCompleteness::Complete
    );
    assert_eq!(model.histogram.uncertain_items.len(), 1);
    assert_eq!(model.histogram.bins[0].members.len(), 1);
    assert_eq!(model.histogram.bins[0].bytes, Weight::LowerBound(10));
    assert_eq!(model.histogram.total_bytes, f03_total(&model));
    assert_eq!(f03_total(&model), Weight::LowerBound(10));
    Ok(())
}
