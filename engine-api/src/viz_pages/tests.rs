use super::*;
use du_core_contracts::result::ResultCompleteness;
use std::sync::{
    Barrier,
    atomic::{AtomicU64, Ordering},
};
struct Ids(AtomicU64);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        let mut v = [0; 16];
        v[8..].copy_from_slice(&self.0.fetch_add(1, Ordering::SeqCst).to_be_bytes());
        Id::from_v7_bytes(v)
    }
}
fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

#[test]
fn retirement_keeps_source_alive_after_the_session_lock() -> Result<(), String> {
    let source = IngestSink::default();
    let mut build = source.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let old = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = ModelPages::new(
        source.clone(),
        NonZeroUsize::MIN,
        Arc::new(Ids(AtomicU64::new(1))),
    );
    let reference = pages
        .register(ModelInput {
            version: old.clone(),
            condition_fingerprint: "old".into(),
            rows: vec![],
            groups: HashMap::new(),
        })
        .map_err(|e| format!("{e:?}"))?;
    let weak = Arc::downgrade(&old);
    let mut build = source.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut retained = None;
    let mut state = pages.state.lock().map_err(|_| "cache lock")?;
    let retired =
        remove_deferred(&mut state, &reference, &mut retained).map_err(|e| format!("{e:?}"))?;
    drop(old);
    assert!(weak.upgrade().is_some());
    drop(state);
    drop(retired);
    drop(retained);
    assert!(weak.upgrade().is_none());
    Ok(())
}

#[test]
fn another_lineage_can_publish_while_rows_are_being_prepared() -> Result<(), String> {
    use du_core_contracts::{
        PathBytes, PathEncoding,
        result::{ResultNativePath, ResultScopeRoot},
    };
    let source = IngestSink::default();
    let first = source
        .bind_current_analysis("first", id(10)?)
        .map_err(|e| format!("{e:?}"))?;
    let second = source
        .bind_current_analysis("second", id(11)?)
        .map_err(|e| format!("{e:?}"))?;
    let root = || ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut build = source
        .open_forest_in_lineage(id(1)?, vec![root()], first)
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = Arc::new(ModelPages::new(
        source.clone(),
        NonZeroUsize::MIN,
        Arc::new(Ids(AtomicU64::new(1))),
    ));
    let entered = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let worker = {
        let pages = pages.clone();
        let entered = entered.clone();
        let resume = resume.clone();
        std::thread::spawn(move || {
            pages.register_after_prepare(
                ModelInput {
                    version,
                    condition_fingerprint: "first".into(),
                    rows: vec![],
                    groups: HashMap::new(),
                },
                None,
                &mut || false,
                || {
                    entered.wait();
                    resume.wait();
                },
            )
        })
    };
    entered.wait();
    let mut build = source
        .open_forest_in_lineage(id(3)?, vec![root()], second)
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    resume.wait();
    assert!(worker.join().map_err(|_| "worker panic")?.is_ok());
    Ok(())
}

#[test]
fn replacement_before_registration_cannot_commit_old_model() -> Result<(), String> {
    let source = IngestSink::default();
    let mut build = source.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let old = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = Arc::new(ModelPages::new(
        source.clone(),
        NonZeroUsize::MIN,
        Arc::new(Ids(AtomicU64::new(1))),
    ));
    let input = ModelInput {
        version: old,
        condition_fingerprint: "old".into(),
        rows: vec![],
        groups: HashMap::new(),
    };
    let entered = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let worker = {
        let pages = pages.clone();
        let entered = entered.clone();
        let resume = resume.clone();
        std::thread::spawn(move || {
            pages.register_with_layout(input, None, &mut || {
                entered.wait();
                resume.wait();
                false
            })
        })
    };
    entered.wait();
    let mut build = source.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let current = build
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    resume.wait();
    assert_eq!(
        worker.join().map_err(|_| "worker panic")?.err(),
        Some(Error::StaleVersion)
    );
    let reference = pages
        .register(ModelInput {
            version: current.clone(),
            condition_fingerprint: "new".into(),
            rows: vec![],
            groups: HashMap::new(),
        })
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        pages
            .rows(&VizRowsRequest {
                model_ref: reference,
                result_revision: current.version_id().clone(),
                start: 0,
                count: 1
            })
            .is_ok()
    );
    Ok(())
}
#[test]
fn admitted_page_survives_model_eviction_and_source_replacement() -> Result<(), String> {
    let source = IngestSink::default();
    let mut b = source.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let a = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = Arc::new(ModelPages::new(
        source.clone(),
        NonZeroUsize::new(2).ok_or("zero")?,
        Arc::new(Ids(AtomicU64::new(1))),
    ));
    let expected: VizRow = serde_json::from_value(serde_json::json!({
        "itemRef": {"resultVersion": a.version_id(), "itemNo": 0},
        "groupRef": null,
        "displayName": "original root",
        "size": {"basis": "logical", "evidence": "measured", "state": "exact", "bytes": "5"},
        "ratio": null,
        "denominator": {"basis": "scope", "value": {"unit": "bytes", "state": "exact", "value": "5"}},
        "legendClassKey": "root",
        "displayFlags": []
    })).map_err(|e| e.to_string())?;
    let model = pages
        .register(ModelInput {
            version: a.clone(),
            condition_fingerprint: "a".into(),
            rows: vec![expected.clone()],
            groups: Default::default(),
        })
        .map_err(|e| format!("{e:?}"))?;
    let start = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let thread = {
        let p = pages.clone();
        let x = start.clone();
        let y = resume.clone();
        let request = VizRowsRequest {
            model_ref: model.clone(),
            result_revision: id(2)?,
            start: 0,
            count: 1,
        };
        std::thread::spawn(move || {
            p.rows_at_admission(&request, || {
                x.wait();
                y.wait();
            })
        })
    };
    start.wait();
    pages.release_model(&model).map_err(|e| format!("{e:?}"))?;
    let expired = pages.rows(&VizRowsRequest {
        model_ref: model.clone(),
        result_revision: id(2)?,
        start: 0,
        count: 1,
    });
    let mut b = source.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let _ = b
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    resume.wait();
    let page = thread
        .join()
        .map_err(|_| "panic")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.result_revision, *a.version_id());
    assert_eq!(page.rows, vec![expected]);
    assert_eq!(expired.err(), Some(Error::ModelExpired));
    assert_eq!(
        pages
            .rows(&VizRowsRequest {
                model_ref: model,
                result_revision: id(2)?,
                start: 0,
                count: 1
            })
            .err(),
        Some(Error::StaleVersion)
    );
    Ok(())
}

#[test]
fn admitted_group_page_survives_eviction_and_replacement() -> Result<(), String> {
    use du_analysis_index::visualization::live_models::{GroupRows, SizeRange};
    let source = IngestSink::default();
    let mut b = source.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let a = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = Arc::new(ModelPages::new(
        source.clone(),
        NonZeroUsize::new(2).ok_or("zero")?,
        Arc::new(Ids(AtomicU64::new(1))),
    ));
    let expected: VizRow = serde_json::from_value(serde_json::json!({"itemRef":{"resultVersion":a.version_id(),"itemNo":0},"groupRef":null,"displayName":"member","size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"5"},"ratio":null,"denominator":{"basis":"scope","value":{"unit":"bytes","state":"exact","value":"5"}},"legendClassKey":"x","displayFlags":[]})).map_err(|e| e.to_string())?;
    let group = GroupRows {
        rows: vec![expected.clone()],
        total: expected.size.clone(),
        size_range: SizeRange::Unknown,
    };
    let model = pages
        .register(ModelInput {
            version: a.clone(),
            condition_fingerprint: "a".into(),
            rows: vec![],
            groups: HashMap::from([("g".into(), group)]),
        })
        .map_err(|e| format!("{e:?}"))?;
    let start = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let thread = {
        let p = pages.clone();
        let s = start.clone();
        let r = resume.clone();
        let m = model.clone();
        std::thread::spawn(move || {
            p.group_rows_at_admission(
                &m,
                "g",
                0,
                1,
                || {
                    s.wait();
                    r.wait();
                },
                &mut || false,
            )
        })
    };
    start.wait();
    pages.release_model(&model).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        pages.group_rows(&model, "g", 0, 1).err(),
        Some(Error::ModelExpired)
    );
    let mut b = source.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let _ = b
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    resume.wait();
    let page = thread
        .join()
        .map_err(|_| "panic")?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.rows, vec![expected]);
    assert_eq!(page.result_revision, *a.version_id());
    Ok(())
}

mod cache_cancellation;
