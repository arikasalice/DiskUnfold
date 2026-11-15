//! A fully read folder of a result sealed Missing renders: the model carries the same
//! version-wide completeness as the response frame and the rows page (F03 FR5.HD1).
use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
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
fn e<T: std::fmt::Debug>(error: T) -> String {
    format!("{error:?}")
}
fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}
fn file(bytes: u64) -> Option<SizeValue> {
    Some(SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    })
}

/// Folder "a" has an unreadable child; folder "s" was fully read. Sealed Missing.
fn missing_result(sink: &IngestSink) -> Result<Arc<ResultVersion>, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut b = sink.open_forest(id(1)?, vec![root]).map_err(e)?;
    b.add_directory_at(0, vec![]).map_err(e)?;
    for parts in [&["a"][..], &["a", "bad"], &["s"]] {
        b.add_directory_at(0, path(parts)).map_err(e)?;
    }
    for (parts, bytes) in [(&["a", "af"][..], 10), (&["s", "x"], 25), (&["s", "y"], 75)] {
        b.add_file(Observation {
            path: path(parts),
            size: file(bytes),
        })
        .map_err(e)?;
    }
    b.record_gap_at(0, path(&["a", "bad"]), "scan.path_denied".into(), None)
        .map_err(e)?;
    b.seal(id(2)?, ResultCompleteness::Missing).map_err(e)
}

#[test]
fn a_fully_read_folder_of_a_missing_result_registers_with_the_version_completeness()
-> Result<(), String> {
    let sink = IngestSink::default();
    let version = missing_result(&sink)?;
    assert_eq!(version.completeness(), ResultCompleteness::Missing);
    let folder = version
        .items()
        .iter()
        .find(|item| item.name == b"s")
        .ok_or("folder s")?;
    let parent = ItemRef {
        result_version: version.version_id().clone(),
        item_no: folder.item_no,
    };
    let desc = typed_layout::treemap_descriptor();
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(desc.clone(), Box::new(Treemap))
        .map_err(e)?;
    let dispatcher = Dispatcher::new(builder.build(&[desc]).map_err(e)?);
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
        },
        &mut || false,
    )
    .map_err(e)?;
    assert_eq!(model.completeness, version.completeness());
    assert_eq!(
        model.totals.scope.amount,
        SizeAmount::Exact { bytes: 100.into() }
    );
    let presentations = model
        .rows
        .iter()
        .map(|r| RowPresentation {
            item: r.item.clone(),
            legend_class_key: "source.class".into(),
            display_flags: vec![],
        })
        .collect();
    let pages = ModelPages::new(
        sink,
        NonZeroUsize::new(3).ok_or("zero")?,
        Arc::new(Ids(AtomicU64::new(10))),
    );
    let reference = production::register_projection(
        &pages,
        version.clone(),
        &model,
        "canonical".into(),
        presentations,
        &mut || false,
    )
    .map_err(e)?;
    let page = pages
        .rows(&VizRowsRequest {
            model_ref: reference,
            result_revision: version.version_id().clone(),
            start: 0,
            count: 10,
        })
        .map_err(e)?;
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.completeness, version.completeness());
    Ok(())
}
