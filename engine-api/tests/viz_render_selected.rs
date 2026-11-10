//! Exercise live model registration from the selected scan through the strategy registry.
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    visualization::{bundle, geometry::Rect},
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    visualization::rows::VizRowsRequest,
};
use du_engine_api::{
    current_analysis,
    viz_pages::{
        self, ModelPages,
        render::{ChildEvidence, Error, Request},
    },
};
use du_job_runtime::JobIdentity;

struct Identity(AtomicU8);
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        id(self.0.fetch_add(1, Ordering::SeqCst))
    }
}
fn id(value: u8) -> Id {
    Id::from_v7_bytes([value; 16])
}
fn publish(sink: &IngestSink, scope: Id, revision: Id) -> Result<(), String> {
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|error| format!("{error:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut scan = sink
        .open_forest_in_lineage(scope, vec![root], lineage)
        .map_err(|error| format!("{error:?}"))?;
    scan.add_directory(vec![])
        .map_err(|error| format!("{error:?}"))?;
    for (name, bytes) in [(b'a', 25_u64), (b'b', 75)] {
        scan.add_file(Observation {
            path: vec![vec![name]],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: bytes.into(),
                },
            }),
        })
        .map_err(|error| format!("{error:?}"))?;
    }
    scan.seal(revision, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}
fn evidence(version: &ResultVersion, revision: &Id) -> Vec<ChildEvidence> {
    version
        .items()
        .iter()
        .filter(|item| item.parent == Some(0))
        .map(|item| ChildEvidence {
            item: ItemRef {
                result_version: revision.clone(),
                item_no: item.item_no,
            },
            path_compare_key: item.name.clone(),
            legend_class_key: "fixture.class".into(),
            display_flags: vec![],
        })
        .collect()
}
fn request(parent: &ItemRef, right: u32) -> Request<'_> {
    Request {
        selected_revision: &parent.result_version,
        requested_revision: None,
        parent,
        basis: SizeBasis::Logical,
        bounds: Rect {
            left: 0,
            top: 0,
            right,
            bottom: 1_000_000,
        },
        format: "treemap",
        strategy: None,
        max_shapes: 10,
    }
}

#[test]
fn selected_render_uses_the_same_rows_and_stable_normalized_reference() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let pages = ModelPages::new(
        sink,
        NonZeroUsize::MIN.saturating_add(3),
        Arc::new(Identity(AtomicU8::new(11))),
    );
    let dispatcher = bundle::implemented_dispatcher().map_err(|error| format!("{error:?}"))?;
    let render = |width, reverse| {
        pages
            .render_selected(
                &dispatcher,
                request(&parent, width),
                |source, _| {
                    let mut rows = evidence(source.version(), source.result_version());
                    if reverse {
                        rows.reverse();
                    }
                    Ok(rows)
                },
                &mut || false,
            )
            .map_err(|error| format!("{error:?}"))
    };
    let first = render(1_000_000, false)?;
    let repeated = render(1_000_000, true)?;
    assert_eq!(first.model_ref, repeated.model_ref);
    assert_eq!(first.model, repeated.model);
    assert!(Arc::ptr_eq(&first.model, &repeated.model));
    assert_ne!(first.model_ref, render(500_000, false)?.model_ref);
    let fallback = first.fallback();
    let page = pages
        .rows(&VizRowsRequest {
            model_ref: fallback.model_ref,
            result_revision: fallback.result_revision,
            start: 0,
            count: 10,
        })
        .map_err(|error| format!("{error:?}"))?;
    for shape in &first.model.shapes {
        assert_eq!(
            page.rows[shape.row_index].item_ref.as_ref(),
            Some(&first.model.rows[shape.row_index].item)
        );
    }
    Ok(())
}

#[test]
fn selected_render_rejects_replaced_or_mismatched_sources() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let pages = ModelPages::new(
        sink.clone(),
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
    );
    let dispatcher = bundle::implemented_dispatcher().map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        pages
            .render_selected(
                &dispatcher,
                request(&parent, 1_000_000),
                |source, _| {
                    let mut rows = evidence(source.version(), source.result_version());
                    rows[0].item.result_version = id(7);
                    Ok(rows)
                },
                &mut || false
            )
            .err(),
        Some(Error::InvalidEvidence)
    );
    assert_eq!(
        pages
            .render_selected(
                &dispatcher,
                request(&parent, 1_000_000),
                |source, _| {
                    publish(&sink, id(4), id(5)).map_err(|_| Error::InvalidEvidence)?;
                    Ok(evidence(source.version(), source.result_version()))
                },
                &mut || false
            )
            .err(),
        Some(Error::Registration(viz_pages::production::Error::Pages(
            viz_pages::Error::StaleVersion
        )))
    );
    assert_eq!(
        pages
            .render_selected(
                &dispatcher,
                request(&parent, 1_000_000),
                |_, _| Err(Error::InvalidEvidence),
                &mut || false
            )
            .err(),
        Some(Error::Source(current_analysis::Error::StaleVersion))
    );
    Ok(())
}
