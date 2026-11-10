use super::*;
use du_engine_api::viz_pages::render::{CommittedRender, FallbackReason};

fn prepared_fixture() -> Result<
    (
        IngestSink,
        ModelPages,
        du_analysis_index::visualization::typed_layout::Dispatcher,
        ItemRef,
    ),
    String,
> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let pages = ModelPages::new(
        sink.clone(),
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
    );
    let dispatcher = bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?;
    Ok((
        sink,
        pages,
        dispatcher,
        ItemRef {
            result_version: id(3),
            item_no: 0,
        },
    ))
}

#[test]
fn preparing_does_not_publish_and_commit_reuses_the_sync_model() -> Result<(), String> {
    let (_, pages, dispatcher, parent) = prepared_fixture()?;
    let prepare = || {
        pages
            .prepare_selected(
                &dispatcher,
                request(&parent, 1000),
                |s, _| Ok(evidence(s.version(), s.result_version())),
                &mut || false,
            )
            .map_err(|e| format!("{e:?}"))
    };
    let candidate = prepare()?;
    assert_eq!(
        pages.rows(&VizRowsRequest {
            model_ref: id(11).as_str().to_owned(),
            result_revision: id(3),
            start: 0,
            count: 10
        }),
        Err(viz_pages::Error::ModelExpired)
    );
    let CommittedRender::Model(first) = pages
        .commit_prepared(candidate, &mut || false)
        .map_err(|e| format!("{e:?}"))?
    else {
        return Err("expected model".into());
    };
    assert_eq!(first.model_ref, id(11).as_str());
    let second = pages
        .render_selected(
            &dispatcher,
            request(&parent, 1000),
            |s, _| Ok(evidence(s.version(), s.result_version())),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.model_ref, second.model_ref);
    assert!(Arc::ptr_eq(&first.model, &second.model));
    Ok(())
}

#[test]
fn replaced_or_cancelled_candidate_cannot_publish() -> Result<(), String> {
    let (sink, pages, dispatcher, parent) = prepared_fixture()?;
    let prepare = || {
        pages
            .prepare_selected(
                &dispatcher,
                request(&parent, 1000),
                |s, _| Ok(evidence(s.version(), s.result_version())),
                &mut || false,
            )
            .map_err(|e| format!("{e:?}"))
    };
    let cancelled = prepare()?;
    assert!(matches!(
        pages.commit_prepared(cancelled, &mut || true),
        Err(Error::Cancelled)
    ));
    let stale = prepare()?;
    publish(&sink, id(4), id(5))?;
    assert!(matches!(
        pages.commit_prepared(stale, &mut || false),
        Err(Error::Registration(viz_pages::production::Error::Pages(
            viz_pages::Error::StaleVersion
        )))
    ));
    Ok(())
}

#[test]
fn candidate_cannot_be_committed_to_another_registry() -> Result<(), String> {
    let (sink, pages, dispatcher, parent) = prepared_fixture()?;
    let candidate = pages
        .prepare_selected(
            &dispatcher,
            request(&parent, 1000),
            |s, _| Ok(evidence(s.version(), s.result_version())),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    let other = ModelPages::new(
        sink,
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(50))),
    );
    assert!(matches!(
        other.commit_prepared(candidate, &mut || false),
        Err(Error::Cache(viz_pages::Error::InvalidModel))
    ));
    Ok(())
}

#[test]
fn prepared_fallback_is_not_published_until_commit() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 25])?;
    let pages = ModelPages::new(
        sink,
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
    );
    let dispatcher = bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let candidate = pages
        .prepare_selected(
            &dispatcher,
            request(&parent, 1000),
            |s, _| {
                let mut rows = evidence(s.version(), s.result_version());
                rows[0].path_compare_key = None;
                Ok(rows)
            },
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    let rows = VizRowsRequest {
        model_ref: id(11).as_str().to_owned(),
        result_revision: id(3),
        start: 0,
        count: 10,
    };
    assert_eq!(pages.rows(&rows), Err(viz_pages::Error::ModelExpired));
    let fallback = match pages.commit_prepared(candidate, &mut || false) {
        Ok(CommittedRender::Fallback {
            reason: FallbackReason::OrderingUnavailable,
            fallback,
        }) => fallback,
        _ => return Err("expected ordering fallback".into()),
    };
    assert_eq!(fallback.model_ref, rows.model_ref);
    assert_eq!(
        pages.rows(&rows).map_err(|e| format!("{e:?}"))?.total_count,
        2
    );
    Ok(())
}

#[test]
fn prepared_unattributed_rows_can_be_cancelled_then_published() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|e| format!("{e:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut scan = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|e| format!("{e:?}"))?;
    scan.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    scan.add_file(Observation {
        path: vec![b"unknown".to_vec()],
        size: None,
    })
    .map_err(|e| format!("{e:?}"))?;
    scan.publish_partial(id(3)).map_err(|e| format!("{e:?}"))?;
    let pages = ModelPages::new(
        sink,
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
    );
    let dispatcher = bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let prepare = || {
        pages
            .prepare_selected(
                &dispatcher,
                request(&parent, 1000),
                |s, _| Ok(evidence(s.version(), s.result_version())),
                &mut || false,
            )
            .map_err(|e| format!("{e:?}"))
    };
    assert!(matches!(
        pages.commit_prepared(prepare()?, &mut || true),
        Err(Error::Cancelled)
    ));
    let fallback = match pages.commit_prepared(prepare()?, &mut || false) {
        Ok(CommittedRender::Fallback {
            reason: FallbackReason::NoAttribution,
            fallback,
        }) => fallback,
        _ => return Err("expected attribution fallback".into()),
    };
    assert_eq!(fallback.result_revision, id(3));
    assert_eq!(
        pages
            .rows(&VizRowsRequest {
                model_ref: fallback.model_ref,
                result_revision: id(3),
                start: 0,
                count: 10
            })
            .map_err(|e| format!("{e:?}"))?
            .total_count,
        1
    );
    Ok(())
}
