use super::*;

#[test]
fn cancellation_while_waiting_for_source_preserves_the_existing_cache() -> Result<(), String> {
    use std::sync::{atomic::AtomicBool, mpsc};
    use std::time::Duration;

    let source = IngestSink::default();
    let mut build = source.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = Arc::new(ModelPages::new(
        source.clone(),
        NonZeroUsize::MIN,
        Arc::new(Ids(AtomicU64::new(30))),
    ));
    let input = |fingerprint: &str| ModelInput {
        version: version.clone(),
        condition_fingerprint: fingerprint.into(),
        rows: vec![],
        groups: HashMap::new(),
    };
    let original = pages
        .register(input("original"))
        .map_err(|e| format!("{e:?}"))?;
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder = {
        let source = source.clone();
        let version = version.clone();
        std::thread::spawn(move || {
            source.with_current(&version, || {
                let _ = held_tx.send(());
                release_rx.recv_timeout(Duration::from_secs(5))
            })
        })
    };
    held_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("{e:?}"))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let (entered_tx, entered_rx) = mpsc::channel();
    let worker = {
        let pages = pages.clone();
        let cancelled = cancelled.clone();
        let candidate = input("candidate");
        std::thread::spawn(move || {
            let mut calls = 0;
            pages.register_with_layout(candidate, None, &mut || {
                calls += 1;
                if calls == 1 {
                    let _ = entered_tx.send(());
                }
                cancelled.load(Ordering::Acquire)
            })
        })
    };
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("{e:?}"))?;
    cancelled.store(true, Ordering::Release);
    release_tx.send(()).map_err(|e| format!("{e:?}"))?;
    holder
        .join()
        .map_err(|_| "holder panic")?
        .map_err(|e| format!("{e:?}"))?
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        worker.join().map_err(|_| "worker panic")?,
        Err(Error::Cancelled)
    ));
    let state = pages.state.lock().map_err(|_| "cache lock")?;
    assert_eq!(state.entries.len(), 1);
    assert!(state.entries.contains_key(&original));
    assert_eq!(state.lru.iter().collect::<Vec<_>>(), vec![&original]);
    Ok(())
}

#[test]
fn cancelled_during_cached_source_admission_preserves_existing_model() -> Result<(), String> {
    use crate::viz_pages::render::{ChildEvidence, Error as RenderError, Request};
    use du_analysis_index::{
        Observation,
        visualization::{bundle, geometry::Rect},
    };
    use du_core_contracts::{
        ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
        result::{ResultNativePath, ResultScopeRoot},
    };
    use std::sync::{atomic::AtomicBool, mpsc};
    use std::time::Duration;

    let source = IngestSink::default();
    let lineage = source
        .bind_current_analysis("cache-test", id(10)?)
        .map_err(|e| format!("{e:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = source
        .open_forest_in_lineage(id(1)?, vec![root], lineage)
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"child".to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact { bytes: 5.into() },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let parent = ItemRef {
        result_version: id(2)?,
        item_no: 0,
    };
    let pages = Arc::new(ModelPages::new(
        source.clone(),
        NonZeroUsize::MIN,
        Arc::new(Ids(AtomicU64::new(30))),
    ));
    let dispatcher = Arc::new(bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?);
    fn request(parent: &ItemRef) -> Request<'_> {
        Request {
            selected_revision: &parent.result_version,
            requested_revision: None,
            parent,
            basis: SizeBasis::Logical,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 1000,
                bottom: 1000,
            },
            format: "treemap",
            strategy: None,
            max_shapes: 10,
        }
    }
    let facts = |source: &crate::current_analysis::CurrentAnalysis| {
        vec![ChildEvidence {
            item: ItemRef {
                result_version: source.result_version().clone(),
                item_no: 1,
            },
            path_compare_key: Some(b"child".to_vec()),
            legend_class_key: "fixture".into(),
            display_flags: vec![],
        }]
    };
    let first = pages
        .render_selected(
            &dispatcher,
            request(&parent),
            |source, _| Ok(facts(source)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let (cache_tx, cache_rx) = mpsc::channel();
    let worker = {
        let pages = pages.clone();
        let dispatcher = dispatcher.clone();
        let parent = parent.clone();
        let cancelled = cancelled.clone();
        std::thread::spawn(move || {
            let mut cache_notified = false;
            pages.render_selected(
                &dispatcher,
                request(&parent),
                |source, _| {
                    let rows = facts(source);
                    entered_tx.send(()).map_err(|_| RenderError::Cancelled)?;
                    resume_rx
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(|_| RenderError::Cancelled)?;
                    Ok(rows)
                },
                &mut || {
                    if !cache_notified
                        && matches!(
                            pages.state.try_lock(),
                            Err(std::sync::TryLockError::WouldBlock)
                        )
                    {
                        cache_notified = true;
                        let _ = cache_tx.send(());
                        return false;
                    }
                    cancelled.load(Ordering::Acquire)
                },
            )
        })
    };
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("{e:?}"))?;
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder = {
        let source = source.clone();
        let version = version.clone();
        std::thread::spawn(move || {
            source
                .with_current(&version, || {
                    let _ = held_tx.send(());
                    release_rx.recv_timeout(Duration::from_secs(5))
                })
                .map_err(|e| format!("{e:?}"))?
                .map_err(|e| format!("{e:?}"))
        })
    };
    held_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("{e:?}"))?;
    resume_tx.send(()).map_err(|e| format!("{e:?}"))?;
    cache_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("{e:?}"))?;
    cancelled.store(true, Ordering::Release);
    release_tx.send(()).map_err(|e| format!("{e:?}"))?;
    holder.join().map_err(|_| "holder panic")??;
    assert!(matches!(
        worker.join().map_err(|_| "worker panic")?,
        Err(RenderError::Cancelled)
    ));
    let next = pages
        .render_selected(
            &dispatcher,
            request(&parent),
            |source, _| Ok(facts(source)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.model_ref, next.model_ref);
    assert!(Arc::ptr_eq(&first.model, &next.model));
    Ok(())
}

#[test]
fn cancellation_during_group_preparation_does_not_publish() -> Result<(), String> {
    use du_analysis_index::visualization::live_models::{GroupRows, SizeRange};
    let source = IngestSink::default();
    let mut builder = source.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    builder
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    let version = builder
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let row: VizRow = serde_json::from_value(serde_json::json!({"itemRef":{"resultVersion":version.version_id(),"itemNo":0},"groupRef":null,"displayName":"member","size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"5"},"ratio":null,"denominator":{"basis":"scope","value":{"unit":"bytes","state":"exact","value":"5"}},"legendClassKey":"x","displayFlags":[]})).map_err(|e| e.to_string())?;
    let size = row.size.clone();
    let group = GroupRows {
        rows: vec![row],
        total: size.clone(),
        size_range: SizeRange::Known {
            min: size.clone(),
            max: size,
        },
    };
    let pages = ModelPages::new(source, NonZeroUsize::MIN, Arc::new(Ids(AtomicU64::new(1))));
    let input = ModelInput {
        version,
        condition_fingerprint: "cancelled".into(),
        rows: vec![],
        groups: HashMap::from([("g".into(), group)]),
    };
    let mut checks = 0;
    let result = pages.register_after_prepare(
        input,
        None,
        &mut || {
            checks += 1;
            checks == 3
        },
        || {},
    );
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(
        pages.state.lock().map_err(|_| "cache lock")?.entries.len(),
        0
    );
    Ok(())
}
