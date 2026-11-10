use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    visualization::{
        geometry::Rect,
        projection::{self, Options, Source},
        registry,
        treemap::ChildKey,
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
    let keys: Vec<_> = version
        .items()
        .iter()
        .filter(|i| i.parent == Some(0))
        .map(|i| ChildKey {
            item: item(i.item_no),
            path_compare_key: i.name.clone(),
        })
        .collect();
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
            keys: &keys,
        },
        &dispatcher,
        Options {
            format: "treemap",
            strategy: None,
            max_shapes: 10,
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
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let page = pages
        .rows(&VizRowsRequest {
            model_ref: reference,
            result_revision: version.version_id().clone(),
            start: 0,
            count: 10,
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.rows.len(), 3);
    assert_eq!(page.result_revision, *version.version_id());
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
            &mut || false
        )
        .err(),
        Some(production::Error::UnsupportedGroups)
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
            &mut || false
        )
        .err(),
        Some(production::Error::SourceMismatch)
    );
    Ok(())
}
