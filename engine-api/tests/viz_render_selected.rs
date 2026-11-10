//! Exercise live model registration from the selected scan through the strategy registry.
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
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
        session::Session,
    },
};
use du_job_runtime::JobIdentity;

struct Identity(AtomicU8);
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        id(self.0.fetch_add(1, Ordering::SeqCst))
    }
}

struct CancelOnNextId {
    ids: Identity,
    armed: AtomicBool,
    cancelled: AtomicBool,
}

impl JobIdentity for CancelOnNextId {
    fn next_id(&self) -> Id {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.cancelled.store(true, Ordering::SeqCst);
        }
        self.ids.next_id()
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
    let dispatcher = bundle::implemented_dispatcher().map_err(|error| format!("{error:?}"))?;
    let session = Session::new(
        sink,
        NonZeroUsize::MIN.saturating_add(3),
        Arc::new(Identity(AtomicU8::new(11))),
        dispatcher,
    );
    let render = |width, reverse| {
        session
            .render_selected(
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
    let page = session
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
fn render_reference_collision_keeps_previous_geometry_and_page() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let source = sink.pin_current(&id(3)).map_err(|e| format!("{e:?}"))?;
    let session = Session::new(
        sink,
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
        bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?,
    );
    let render = |width| {
        session.render_selected(
            request(&parent, width),
            |source, _| Ok(evidence(source.version(), source.result_version())),
            &mut || false,
        )
    };
    let first = render(1000).map_err(|e| format!("{e:?}"))?;
    source
        .insert_viz_rows(id(12).as_str().to_owned(), vec![], Default::default())
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        render(500),
        Err(Error::Registration(viz_pages::production::Error::Pages(
            viz_pages::Error::ReferenceCollision
        )))
    ));
    let page = session
        .rows(&VizRowsRequest {
            model_ref: first.model_ref.clone(),
            result_revision: id(3),
            start: 0,
            count: 10,
        })
        .map_err(|e| format!("{e:?}"))?;
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

#[test]
fn cancelled_model_registration_keeps_the_previous_geometry_and_page() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let identity = Arc::new(CancelOnNextId {
        ids: Identity(AtomicU8::new(11)),
        armed: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
    });
    let pages = ModelPages::new(sink, NonZeroUsize::MIN, identity.clone());
    let dispatcher = bundle::implemented_dispatcher().map_err(|error| format!("{error:?}"))?;
    let render = |width| {
        pages.render_selected(
            &dispatcher,
            request(&parent, width),
            |source, _| Ok(evidence(source.version(), source.result_version())),
            &mut || identity.cancelled.load(Ordering::SeqCst),
        )
    };
    let first = render(1_000_000).map_err(|error| format!("{error:?}"))?;

    identity.armed.store(true, Ordering::SeqCst);
    assert_eq!(render(500_000).err(), Some(Error::Cancelled));
    identity.cancelled.store(false, Ordering::SeqCst);

    let fallback = first.fallback();
    let page = pages
        .rows(&VizRowsRequest {
            model_ref: fallback.model_ref,
            result_revision: fallback.result_revision,
            start: 0,
            count: 10,
        })
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(page.rows.len(), first.model.rows.len());
    let reused = render(1_000_000).map_err(|error| format!("{error:?}"))?;
    assert!(Arc::ptr_eq(&first.model, &reused.model));
    assert_ne!(
        render(500_000)
            .map_err(|error| format!("{error:?}"))?
            .model_ref,
        id(12).as_str()
    );
    Ok(())
}

#[test]
fn session_dispatches_registered_revision_and_rejects_unknown_strategy() -> Result<(), String> {
    use du_analysis_index::visualization::{registry, typed_layout};
    use du_core_contracts::visualization::{LayoutDescriptor, LayoutStrategyKey};
    use std::sync::atomic::AtomicUsize;

    struct Counted(Arc<AtomicUsize>);
    impl typed_layout::Strategy for Counted {
        fn layout(
            &self,
            input: &typed_layout::Input,
            cancelled: &mut dyn FnMut() -> bool,
        ) -> Result<typed_layout::Output, typed_layout::Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            typed_layout::Strategy::layout(&typed_layout::Treemap, input, cancelled)
        }
    }
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = registry::Builder::<Box<dyn typed_layout::Strategy>>::default();
    let descriptor = |revision| LayoutDescriptor {
        format: "treemap".into(),
        strategy: LayoutStrategyKey {
            id: "viz.treemap".into(),
            revision,
        },
    };
    builder
        .register(descriptor(2), Box::new(Counted(calls.clone())))
        .map_err(|e| format!("{e:?}"))?;
    let dispatcher =
        typed_layout::Dispatcher::new(builder.build(&[]).map_err(|e| format!("{e:?}"))?);
    let session = Session::new(
        sink,
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(20))),
        dispatcher,
    );
    let render = || {
        session.render_selected(
            request(&parent, 1000),
            |source, _| Ok(evidence(source.version(), source.result_version())),
            &mut || false,
        )
    };
    let first = render().map_err(|e| format!("{e:?}"))?;
    let second = render().map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.model.strategy, descriptor(2));
    assert_eq!(first.model_ref, second.model_ref);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let missing = LayoutStrategyKey {
        id: "viz.treemap".into(),
        revision: 1,
    };
    let mut unknown = request(&parent, 1000);
    unknown.strategy = Some(&missing);
    assert!(matches!(
        session.render_selected(unknown, |_, _| unreachable!(), &mut || false),
        Err(Error::Strategy(
            du_engine_api::viz_catalog::Error::UnknownStrategy
        ))
    ));
    Ok(())
}

#[test]
fn dropping_a_session_releases_only_its_models_from_the_pinned_source() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let pinned = sink
        .pin_current(&parent.result_version)
        .map_err(|error| format!("{error:?}"))?;
    let ids = Arc::new(Identity(AtomicU8::new(30)));
    let session = || {
        let dispatcher = bundle::implemented_dispatcher().map_err(|error| format!("{error:?}"))?;
        Ok::<_, String>(Session::new(
            sink.clone(),
            NonZeroUsize::MIN,
            ids.clone(),
            dispatcher,
        ))
    };
    let render = |session: &Session| {
        session
            .render_selected(
                request(&parent, 1000),
                |source, _| Ok(evidence(source.version(), source.result_version())),
                &mut || false,
            )
            .map_err(|error| format!("{error:?}"))
    };
    let first = session()?;
    let second = session()?;
    let first_model = render(&first)?;
    let second_model = render(&second)?;
    assert_ne!(first_model.model_ref, second_model.model_ref);
    let first_rows = VizRowsRequest {
        model_ref: first_model.model_ref,
        result_revision: parent.result_version.clone(),
        start: 0,
        count: 1,
    };
    assert!(first.rows(&first_rows).is_ok());
    assert!(matches!(
        second.rows(&first_rows),
        Err(viz_pages::Error::ModelExpired)
    ));

    drop(first);
    assert!(matches!(
        pinned.pin_viz_rows(&first_rows.model_ref),
        Err(du_analysis_index::visualization::live_models::Error::Missing)
    ));
    let second_rows = VizRowsRequest {
        model_ref: second_model.model_ref,
        ..first_rows
    };
    assert_eq!(
        second
            .rows(&second_rows)
            .map_err(|error| format!("{error:?}"))?
            .total_count,
        2
    );
    Ok(())
}
