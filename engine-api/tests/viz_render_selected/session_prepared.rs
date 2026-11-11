use super::*;
use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder},
    viz_pages::{
        commands,
        render::{CommittedRender, FallbackReason},
    },
};
use serde_json::json;

#[test]
fn session_preparation_shares_membership_dispatcher_and_rows_with_sync_and_ipc()
-> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let ids = Arc::new(Identity(AtomicU8::new(11)));
    let session = Arc::new(Session::new(
        sink,
        NonZeroUsize::MIN,
        ids.clone(),
        bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?,
    ));
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let mut seen = Vec::new();
    let candidate = session
        .prepare_selected(
            request(&parent, 1000),
            |source, item, _| {
                assert_eq!(source.result_version(), &id(3));
                assert_eq!(item.parent, Some(0));
                seen.push(item.item_no);
                Ok(policy(item))
            },
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(seen.len(), 2);
    assert_eq!(ids.0.load(Ordering::SeqCst), 11);
    let reserved = session.reserve_prepared(candidate);
    assert_eq!(ids.0.load(Ordering::SeqCst), 12);
    let CommittedRender::Model(first) = session
        .commit_reserved(reserved, &mut || false)
        .map_err(|e| format!("{e:?}"))?
    else {
        return Err("expected geometry".into());
    };
    assert_eq!(ids.0.load(Ordering::SeqCst), 12);
    let sync = session
        .render_selected(
            request(&parent, 1000),
            |_, item, _| Ok(policy(item)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.model_ref, sync.model_ref);
    assert!(Arc::ptr_eq(&first.model, &sync.model));
    let request = VizRowsRequest {
        model_ref: first.model_ref.clone(),
        result_revision: id(3),
        start: 0,
        count: 10,
    };
    let expected = session.rows(&request).map_err(|e| format!("{e:?}"))?;
    assert_eq!(expected.total_count, 2);
    let mut builder = EngineBuilder::default();
    commands::register_rows(&mut builder, session);
    let engine = builder.build().map_err(|e| e.to_string())?;
    let response = engine.call(
        CallEnvelope {
            name: "viz.rows".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: id(20),
            body: json!(request),
        },
        &CallerCtx::new(Channel::Gui, id(20), None),
    );
    assert!(response.error.is_none());
    assert_eq!(response.body, json!(expected));
    Ok(())
}

#[test]
fn session_candidate_rejects_other_owner_cancel_and_replacement() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let new_session = || -> Result<Session, String> {
        Ok(Session::new(
            sink.clone(),
            NonZeroUsize::MIN,
            Arc::new(Identity(AtomicU8::new(11))),
            bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?,
        ))
    };
    let first = new_session()?;
    let second = new_session()?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let prepare = || {
        first
            .prepare_selected(
                request(&parent, 1000),
                |_, item, _| Ok(policy(item)),
                &mut || false,
            )
            .map_err(|e| format!("{e:?}"))
    };
    assert!(matches!(
        second.commit_reserved(first.reserve_prepared(prepare()?), &mut || false),
        Err(Error::Cache(viz_pages::Error::InvalidModel))
    ));
    assert!(matches!(
        first.commit_reserved(first.reserve_prepared(prepare()?), &mut || true),
        Err(Error::Cancelled)
    ));
    let old = first.reserve_prepared(prepare()?);
    publish(&sink, id(4), id(5))?;
    assert!(matches!(
        first.commit_reserved(old, &mut || false),
        Err(Error::Registration(viz_pages::production::Error::Pages(
            viz_pages::Error::StaleVersion
        )))
    ));
    Ok(())
}

#[test]
fn session_fallback_commit_is_success_and_rows_match_sync() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 25])?;
    let session = Arc::new(Session::new(
        sink,
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
        bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?,
    ));
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let evidence = |_: &du_engine_api::current_analysis::CurrentAnalysis,
                    item: &du_core_contracts::result::ResultItem,
                    _: &mut dyn FnMut() -> bool| {
        let mut value = policy(item);
        value.path_compare_key = None;
        Ok(value)
    };
    let candidate = session
        .prepare_selected(request(&parent, 1000), evidence, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let CommittedRender::Fallback {
        reason: FallbackReason::OrderingUnavailable,
        fallback,
    } = session
        .commit_prepared(candidate, &mut || false)
        .map_err(|e| format!("{e:?}"))?
    else {
        return Err("expected fallback".into());
    };
    let sync = match session.render_selected(request(&parent, 1000), evidence, &mut || false) {
        Err(Error::OrderingUnavailable(value)) => value,
        _ => return Err("expected sync fallback".into()),
    };
    assert_eq!(fallback, sync);
    let rows_request = VizRowsRequest {
        model_ref: fallback.model_ref,
        result_revision: fallback.result_revision,
        start: 0,
        count: 10,
    };
    let rows = session.rows(&rows_request).map_err(|e| format!("{e:?}"))?;
    assert_eq!(rows.total_count, 2);
    let mut builder = EngineBuilder::default();
    commands::register_rows(&mut builder, session);
    let engine = builder.build().map_err(|e| e.to_string())?;
    let response = engine.call(
        CallEnvelope {
            name: "viz.rows".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: id(20),
            body: json!(rows_request),
        },
        &CallerCtx::new(Channel::Gui, id(20), None),
    );
    assert!(response.error.is_none());
    assert_eq!(response.body, json!(rows));
    Ok(())
}

#[test]
fn deferred_commit_retains_evicted_layout_until_owner_drops() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let session = Session::new(
        sink,
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
        bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?,
    );
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let first = session
        .render_selected(
            request(&parent, 1000),
            |_, item, _| Ok(policy(item)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    let old = Arc::downgrade(&first.model);
    drop(first);
    let candidate = session
        .prepare_selected(
            request(&parent, 2000),
            |_, item, _| Ok(policy(item)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    let reserved = session.reserve_prepared(candidate);
    let mut deferred = session.defer_reserved(reserved);
    let committed = session
        .commit_deferred(&mut deferred, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(committed, CommittedRender::Model(_)));
    assert!(old.upgrade().is_some());
    assert!(
        session
            .commit_deferred(&mut deferred, &mut || false)
            .is_err()
    );
    drop(deferred);
    assert!(old.upgrade().is_none());
    Ok(())
}

#[test]
fn deferred_cancel_stale_and_cache_hit_preserve_prior_rows() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let session = Session::new(
        sink.clone(),
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(20))),
        bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?,
    );
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let prepare = || {
        session
            .prepare_selected(
                request(&parent, 1000),
                |_, item, _| Ok(policy(item)),
                &mut || false,
            )
            .map_err(|e| format!("{e:?}"))
    };
    let mut cancelled = session.defer_reserved(session.reserve_prepared(prepare()?));
    assert!(matches!(
        session.commit_deferred(&mut cancelled, &mut || true),
        Err(Error::Cancelled)
    ));
    let first = session
        .render_selected(
            request(&parent, 1000),
            |_, item, _| Ok(policy(item)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    let mut hit = session.defer_reserved(session.reserve_prepared(prepare()?));
    let CommittedRender::Model(cached) = session
        .commit_deferred(&mut hit, &mut || false)
        .map_err(|e| format!("{e:?}"))?
    else {
        return Err("expected model".into());
    };
    assert_eq!(cached.model_ref, first.model_ref);
    assert!(Arc::ptr_eq(&cached.model, &first.model));
    let mut stale = session.defer_reserved(session.reserve_prepared(prepare()?));
    publish(&sink, id(4), id(5))?;
    assert!(matches!(
        session.commit_deferred(&mut stale, &mut || false),
        Err(Error::Registration(viz_pages::production::Error::Pages(
            viz_pages::Error::StaleVersion
        )))
    ));
    Ok(())
}
