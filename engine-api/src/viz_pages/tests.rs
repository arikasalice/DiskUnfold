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
            groups: HashSet::new(),
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
