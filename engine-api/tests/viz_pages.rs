use du_analysis_index::{IngestSink, Observation, ResultVersion};
use du_core_contracts::{
    Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
    visualization::rows::{VizRow, VizRowsRequest},
};
use du_engine_api::viz_pages::{Error, ModelInput, ModelPages};
use du_job_runtime::JobIdentity;
use std::{
    collections::HashSet,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
struct Ids(AtomicU64);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        let mut b = [0; 16];
        b[8..].copy_from_slice(&self.0.fetch_add(1, Ordering::SeqCst).to_be_bytes());
        Id::from_v7_bytes(b)
    }
}
fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn add(b: &mut du_analysis_index::BuildHandle, n: u8) -> Result<(), String> {
    b.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    b.add_file(Observation {
        path: vec![b"x".to_vec()],
        size: Some(SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount: SizeAmount::Exact {
                bytes: u64::from(n).into(),
            },
        }),
    })
    .map_err(|e| format!("{e:?}"))
}
fn make_row(v: &ResultVersion) -> Result<VizRow, String> {
    serde_json::from_value(serde_json::json!({"itemRef":{"resultVersion":v.version_id(),"itemNo":1},"groupRef":null,"displayName":"item","size":{"basis":"logical","evidence":"measured","state":"exact","bytes":"5"},"ratio":null,"denominator":{"basis":"scope","value":{"unit":"bytes","state":"exact","value":"5"}},"legendClassKey":"x","displayFlags":[]})).map_err(|e|format!("{e:?}"))
}
fn request(r: &str, v: &ResultVersion) -> VizRowsRequest {
    VizRowsRequest {
        model_ref: r.into(),
        result_revision: v.version_id().clone(),
        start: 0,
        count: 1,
    }
}
fn register(p: &ModelPages, v: &Arc<ResultVersion>, key: &str) -> Result<String, String> {
    p.register(ModelInput {
        version: v.clone(),
        condition_fingerprint: key.into(),
        rows: vec![make_row(v)?],
        groups: HashSet::new(),
    })
    .map_err(|e| format!("{e:?}"))
}
fn pages(s: IngestSink) -> Result<ModelPages, String> {
    Ok(ModelPages::new(
        s,
        NonZeroUsize::new(2).ok_or("capacity")?,
        Arc::new(Ids(AtomicU64::new(10))),
    ))
}
#[test]
fn current_replacement_restore_and_model_expiry_are_distinct() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    add(&mut b, 5)?;
    let a = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let p = pages(sink.clone())?;
    let reference = register(&p, &a, "a")?;
    let accepted = p
        .rows(&request(&reference, &a))
        .map_err(|e| format!("{e:?}"))?;
    let mut b = sink.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    add(&mut b, 9)?;
    let partial = b.publish_partial(id(4)?).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        p.rows(&request(&reference, &a)).err(),
        Some(Error::StaleVersion)
    );
    assert_eq!(accepted.rows[0].size, make_row(&a)?.size);
    assert_eq!(
        p.register(ModelInput {
            version: a.clone(),
            condition_fingerprint: "late".into(),
            rows: vec![],
            groups: HashSet::new()
        })
        .err(),
        Some(Error::StaleVersion)
    );
    let bref = register(&p, &partial, "b")?;
    b.abort();
    assert!(p.rows(&request(&reference, &a)).is_ok());
    assert_eq!(
        p.rows(&request(&bref, &partial)).err(),
        Some(Error::StaleVersion)
    );
    p.release_model(&reference).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        p.rows(&request(&reference, &a)).err(),
        Some(Error::ModelExpired)
    );
    Ok(())
}
#[test]
fn weak_index_does_not_keep_source_or_its_models_alive() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    add(&mut b, 5)?;
    let a = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let p = pages(sink.clone())?;
    let reference = register(&p, &a, "a")?;
    let weak = Arc::downgrade(&a);
    let mut next = sink.open(id(3)?).map_err(|e| format!("{e:?}"))?;
    add(&mut next, 9)?;
    let b = next
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    drop(a);
    assert!(weak.upgrade().is_none());
    assert_eq!(
        p.rows(&VizRowsRequest {
            model_ref: reference,
            result_revision: id(2)?,
            start: 0,
            count: 1
        })
        .err(),
        Some(Error::StaleVersion)
    );
    assert!(register(&p, &b, "b").is_ok());
    Ok(())
}
#[test]
fn same_id_from_another_store_cannot_be_admitted() -> Result<(), String> {
    let one = IngestSink::default();
    let two = IngestSink::default();
    let mut b = one.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    add(&mut b, 5)?;
    let _ = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut b = two.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    add(&mut b, 5)?;
    let impostor = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let p = pages(one)?;
    assert_eq!(
        p.register(ModelInput {
            version: impostor,
            condition_fingerprint: "x".into(),
            rows: vec![],
            groups: HashSet::new()
        })
        .err(),
        Some(Error::StaleVersion)
    );
    Ok(())
}

#[test]
fn another_lineage_does_not_replace_the_target_lineage() -> Result<(), String> {
    use du_core_contracts::{
        PathBytes, PathEncoding,
        result::{ResultNativePath, ResultScopeRoot},
    };
    let sink = IngestSink::default();
    let x = sink
        .bind_current_analysis("first", id(20)?)
        .map_err(|e| format!("{e:?}"))?;
    let y = sink
        .bind_current_analysis("second", id(21)?)
        .map_err(|e| format!("{e:?}"))?;
    let root = || ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&[67, 0]),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut first = sink
        .open_forest_in_lineage(id(22)?, vec![root()], x)
        .map_err(|e| format!("{e:?}"))?;
    add(&mut first, 5)?;
    let a = first
        .seal(id(23)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = pages(sink.clone())?;
    let model = register(&pages, &a, "a")?;
    let mut second = sink
        .open_forest_in_lineage(id(24)?, vec![root()], y)
        .map_err(|e| format!("{e:?}"))?;
    add(&mut second, 9)?;
    let _ = second
        .seal(id(25)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        pages
            .rows(&request(&model, &a))
            .map_err(|e| format!("{e:?}"))?
            .result_revision,
        *a.version_id()
    );
    Ok(())
}
#[test]
fn repeated_source_replacements_release_each_source() -> Result<(), String> {
    let sink = IngestSink::default();
    let p = pages(sink.clone())?;
    let mut weak = Vec::new();
    for n in 1..=12 {
        let mut b = sink.open(id(100 + n * 2)?).map_err(|e| format!("{e:?}"))?;
        add(&mut b, 5)?;
        let v = b
            .seal(id(101 + n * 2)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        let _ = register(&p, &v, &format!("condition{n}"))?;
        weak.push(Arc::downgrade(&v));
    }
    assert!(weak[..11].iter().all(|w| w.upgrade().is_none()));
    assert!(weak[11].upgrade().is_some());
    Ok(())
}
