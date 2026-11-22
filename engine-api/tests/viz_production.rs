use du_analysis_index::{
    FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence, SpaceObservation,
    visualization::{
        geometry::Rect,
        projection::{self, Options, Source},
        registry,
        typed_layout::{self, Dispatcher, Strategy, Treemap},
    },
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    visualization::rows::VizRowsRequest,
};
use du_engine_api::viz_pages::{
    ModelPages,
    production::{self, RowPresentation},
};
use du_job_runtime::JobIdentity;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
struct Ids(AtomicU64);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        let mut bytes = [0; 16];
        bytes[8..].copy_from_slice(&self.0.fetch_add(1, Ordering::SeqCst).to_be_bytes());
        Id::from_v7_bytes(bytes)
    }
}
fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn fixture() -> Result<(IngestSink, Arc<ResultVersion>, projection::Model), String> {
    let sink = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut b = sink
        .open_observed(id(1)?, root)
        .map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, size) in [
        (b"a".as_slice(), 25_u64),
        (b"b".as_slice(), 75),
        (b"zero".as_slice(), 0),
    ] {
        b.add_file(Observation {
            path: vec![name.to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact { bytes: size.into() },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    }
    let version = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let item = |n| ItemRef {
        result_version: version.version_id().clone(),
        item_no: n,
    };
    let parent = item(0);
    let desc = typed_layout::treemap_descriptor();
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(desc.clone(), Box::new(Treemap))
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(builder.build(&[desc]).map_err(|e| format!("{e:?}"))?);
    let model = projection::project(
        Source {
            version: &version,
            parent: &parent,
            basis: SizeBasis::Logical,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 1_000_000,
                bottom: 1_000_000,
            },
        },
        &dispatcher,
        Options {
            format: "treemap",
            strategy: None,
            max_shapes: 10,
            group_tail: true,
        },
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok((sink, version, model))
}

fn hardlink_fixture() -> Result<(IngestSink, Arc<ResultVersion>, projection::Model), String> {
    let sink = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut b = sink
        .open_observed(id(11)?, root)
        .map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![b"files".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let mib = 1024_u64 * 1024;
    for (name, bytes, file_id, link_count) in [
        (b"b".as_slice(), mib, 7_u8, 2_u64),
        (b"a".as_slice(), mib, 7, 2),
        (b"large".as_slice(), 2 * mib, 8, 1),
    ] {
        b.add_with_space(
            Observation {
                path: vec![b"files".to_vec(), name.to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: bytes.into(),
                    },
                }),
            },
            SpaceObservation {
                allocated: Some(SizeValue {
                    basis: SizeBasis::Allocated,
                    evidence: SizeEvidence::OsReported,
                    amount: SizeAmount::Exact {
                        bytes: bytes.into(),
                    },
                }),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![file_id],
                }),
                link_count: Some(link_count),
                sharing: SharingEvidence::Unknown,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    }
    let version = b
        .seal(id(12)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let parent_no = version
        .items()
        .iter()
        .find(|item| item.name == b"files")
        .map(|item| item.item_no)
        .ok_or("missing folder")?;
    let parent = ItemRef {
        result_version: version.version_id().clone(),
        item_no: parent_no,
    };
    let desc = typed_layout::treemap_descriptor();
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(desc.clone(), Box::new(Treemap))
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(builder.build(&[desc]).map_err(|e| format!("{e:?}"))?);
    let model = projection::project(
        Source {
            version: &version,
            parent: &parent,
            basis: SizeBasis::Allocated,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 1_000_000,
                bottom: 1_000_000,
            },
        },
        &dispatcher,
        Options {
            format: "treemap",
            strategy: None,
            max_shapes: 10,
            group_tail: true,
        },
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok((sink, version, model))
}
fn labels(model: &projection::Model) -> Vec<RowPresentation> {
    model
        .rows
        .iter()
        .map(|r| RowPresentation {
            item: r.item.clone(),
            legend_class_key: "source.class".into(),
            display_flags: vec!["source.flag".into()],
        })
        .collect()
}
fn pages(sink: IngestSink) -> Result<ModelPages, String> {
    Ok(ModelPages::new(
        sink,
        NonZeroUsize::new(3).ok_or("zero")?,
        Arc::new(Ids(AtomicU64::new(10))),
    ))
}
#[test]
fn actual_pinned_projection_yields_matching_page_rows_and_geometry() -> Result<(), String> {
    let (sink, version, model) = fixture()?;
    let pages = pages(sink)?;
    let reference = production::register_projection(
        &pages,
        version.clone(),
        &model,
        "canonical".into(),
        labels(&model),
        None,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let page = pages
        .rows(&VizRowsRequest {
            model_ref: reference,
            result_revision: version.version_id().clone(),
            start: 0,
            count: 10,
            parent_shape_ref: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.rows.len(), 3);
    assert_eq!(page.result_revision, *version.version_id());
    assert_eq!(page.size_basis, SizeBasis::Logical);
    for (row, source) in page.rows.iter().zip(&model.rows) {
        assert_eq!(row.item_ref.as_ref(), Some(&source.item));
        assert_eq!(row.size, source.size);
        assert_eq!(
            row.ratio.as_ref().map(|x| x.as_str()),
            source.ratio.as_deref()
        );
        assert_eq!(row.denominator.size.as_ref(), Some(&model.totals.scope));
        assert_eq!(row.legend_class_key, "source.class");
        assert_eq!(row.display_flags, ["source.flag"]);
    }
    for shape in &model.shapes {
        assert_eq!(
            page.rows[shape.row_index].item_ref.as_ref(),
            Some(&model.rows[shape.row_index].item)
        );
    }
    assert!(
        page.rows
            .iter()
            .any(|r| r.display_name == "a" && r.ratio.as_ref().map(|x| x.as_str()) == Some("0.25"))
    );
    assert!(
        page.rows
            .iter()
            .any(|r| r.display_name == "zero" && r.ratio.as_ref().map(|x| x.as_str()) == Some("0"))
    );
    Ok(())
}

#[test]
fn allocated_hardlinks_have_one_area_and_blank_non_counted_ratio() -> Result<(), String> {
    let (sink, version, model) = hardlink_fixture()?;
    assert_eq!(model.rows.len(), 3);
    assert_eq!(model.shapes.len(), 2);
    assert_eq!(model.detail.unknown_items, 0);
    assert_eq!(
        model.detail.unknown_share,
        du_analysis_index::visualization::projection::UnknownShare::None
    );
    assert!(!model.detail.remainder_causes.unattributed);

    let pages = pages(sink)?;
    let reference = production::register_projection(
        &pages,
        version.clone(),
        &model,
        "hardlinks".into(),
        labels(&model),
        None,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let page = pages
        .rows(&VizRowsRequest {
            model_ref: reference,
            result_revision: version.version_id().clone(),
            start: 0,
            count: 10,
            parent_shape_ref: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.size_basis, SizeBasis::Allocated);

    let row = |name: &str| {
        page.rows
            .iter()
            .find(|row| row.display_name == name)
            .ok_or_else(|| format!("missing row {name}"))
    };
    let bytes = |name: &str| -> Result<u64, String> {
        match &row(name)?.size.amount {
            SizeAmount::Exact { bytes } => Ok(bytes.get()),
            _ => Err(format!("unexpected size for {name}")),
        }
    };
    let a = row("a")?;
    let b = row("b")?;
    let large = row("large")?;
    assert_eq!(bytes("a")?, 1024 * 1024);
    assert_eq!(bytes("b")?, 0);
    assert_eq!(bytes("large")?, 2 * 1024 * 1024);
    assert_eq!(a.shares_space, Some(true));
    assert!(a.shared_counted_item.is_none());
    assert_eq!(b.shares_space, Some(true));
    assert_eq!(b.ratio, None);
    assert_eq!(
        b.shared_counted_item.as_ref().map(|item| item.item_no),
        a.item_ref.as_ref().map(|item| item.item_no)
    );
    assert_eq!(large.shares_space, None);
    let ratio_a: f64 = a
        .ratio
        .as_ref()
        .ok_or("missing counted ratio")?
        .as_str()
        .parse()
        .map_err(|e| format!("{e}"))?;
    let ratio_large: f64 = large
        .ratio
        .as_ref()
        .ok_or("missing ordinary ratio")?
        .as_str()
        .parse()
        .map_err(|e| format!("{e}"))?;
    assert!((ratio_a - 1.0 / 3.0).abs() < 0.00001);
    assert!((ratio_large - 2.0 / 3.0).abs() < 0.00001);
    assert!(ratio_a + ratio_large <= 1.0);
    Ok(())
}
#[test]
fn refuses_missing_evidence_forged_denominator_groups_and_cancel() -> Result<(), String> {
    let (sink, version, mut model) = fixture()?;
    let pages = pages(sink)?;
    assert_eq!(
        production::register_projection(
            &pages,
            version.clone(),
            &model,
            "a".into(),
            vec![],
            None,
            &mut || false
        )
        .err(),
        Some(production::Error::MissingClassification)
    );
    assert_eq!(
        production::register_projection(
            &pages,
            version.clone(),
            &model,
            "a".into(),
            labels(&model),
            None,
            &mut || true
        )
        .err(),
        Some(production::Error::Cancelled)
    );
    model.totals.grouped_bytes = 1;
    assert_eq!(
        production::register_projection(
            &pages,
            version.clone(),
            &model,
            "a".into(),
            labels(&model),
            None,
            &mut || false
        )
        .err(),
        Some(production::Error::InvalidGroup)
    );
    model.totals.grouped_bytes = 0;
    model.totals.scope.amount = SizeAmount::Exact {
        bytes: 999_u64.into(),
    };
    assert_eq!(
        production::register_projection(
            &pages,
            version,
            &model,
            "a".into(),
            labels(&model),
            None,
            &mut || false
        )
        .err(),
        Some(production::Error::SourceMismatch)
    );
    Ok(())
}

#[test]
fn strategy_projection_and_page_remain_bound_to_the_admitted_revision() -> Result<(), String> {
    let (sink, version, model) = fixture()?;
    let pages = pages(sink.clone())?;
    let reference = production::register_projection(
        &pages,
        version.clone(),
        &model,
        "bound-condition".into(),
        labels(&model),
        None,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let request = VizRowsRequest {
        model_ref: reference.clone(),
        result_revision: version.version_id().clone(),
        start: 0,
        count: 10,
        parent_shape_ref: None,
    };
    let admitted = pages.rows(&request).map_err(|e| format!("{e:?}"))?;
    assert_eq!(model.strategy, typed_layout::treemap_descriptor());
    assert_eq!(admitted.result_revision, model.result_revision);
    for shape in &model.shapes {
        let row = admitted.rows.get(shape.row_index).ok_or("missing row")?;
        let source = model
            .rows
            .get(shape.row_index)
            .ok_or("missing source row")?;
        assert_eq!(row.item_ref.as_ref(), Some(&source.item));
        assert_eq!(source.item.result_version, model.result_revision);
        assert_eq!(row.size, source.size);
    }

    // Replace the same live lineage while retaining the old model and admitted page.
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut builder = sink
        .open_observed(id(3)?, root)
        .map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    let replacement = builder
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_ne!(replacement.version_id(), version.version_id());
    assert_eq!(
        pages.rows(&request).err(),
        Some(du_engine_api::viz_pages::Error::StaleVersion)
    );
    assert_eq!(
        production::register_projection(
            &pages,
            version.clone(),
            &model,
            "another-condition".into(),
            labels(&model),
            None,
            &mut || false
        )
        .err(),
        Some(production::Error::Pages(
            du_engine_api::viz_pages::Error::StaleVersion
        ))
    );
    // The already admitted page and layout remain unchanged, never rebound to B.
    assert_eq!(admitted.result_revision, *version.version_id());
    for shape in &model.shapes {
        assert_eq!(
            admitted.rows[shape.row_index].item_ref.as_ref(),
            Some(&model.rows[shape.row_index].item)
        );
    }
    Ok(())
}
