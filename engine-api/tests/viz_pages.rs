use du_analysis_index::{IngestSink, Observation, ResultVersion};
use du_core_contracts::{
    Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
    visualization::rows::{VizRow, VizRowsRequest},
};
use du_engine_api::viz_pages::{Error, ModelInput, ModelPages};
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
        groups: Default::default(),
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
fn source_reference_collision_preserves_existing_model_and_page() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    add(&mut build, 5)?;
    let version = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = ModelPages::new(sink, NonZeroUsize::MIN, Arc::new(Ids(AtomicU64::new(10))));
    let old = register(&pages, &version, "old")?;
    let next = Ids(AtomicU64::new(11)).next_id().as_str().to_owned();
    version
        .insert_viz_rows(next, vec![], Default::default())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        pages
            .register(ModelInput {
                version: version.clone(),
                condition_fingerprint: "new".into(),
                rows: vec![make_row(&version)?],
                groups: Default::default()
            })
            .err(),
        Some(Error::ReferenceCollision)
    );
    assert_eq!(
        pages
            .rows(&request(&old, &version))
            .map_err(|e| format!("{e:?}"))?
            .rows
            .len(),
        1
    );
    let new = register(&pages, &version, "new")?;
    assert_eq!(
        pages.rows(&request(&old, &version)).err(),
        Some(Error::ModelExpired)
    );
    assert!(pages.rows(&request(&new, &version)).is_ok());
    Ok(())
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
            groups: Default::default()
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
            groups: Default::default()
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
fn expired_source_is_evicted_before_lru_live_other_lineage() -> Result<(), String> {
    use du_core_contracts::{
        PathBytes, PathEncoding,
        result::{ResultNativePath, ResultScopeRoot},
    };
    let sink = IngestSink::default();
    let a_lineage = sink
        .bind_current_analysis("a", id(20)?)
        .map_err(|e| format!("{e:?}"))?;
    let b_lineage = sink
        .bind_current_analysis("b", id(21)?)
        .map_err(|e| format!("{e:?}"))?;
    let root = || ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&[67, 0]),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut a_build = sink
        .open_forest_in_lineage(id(22)?, vec![root()], a_lineage)
        .map_err(|e| format!("{e:?}"))?;
    add(&mut a_build, 5)?;
    let a = a_build
        .seal(id(23)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut b_build = sink
        .open_forest_in_lineage(id(24)?, vec![root()], b_lineage.clone())
        .map_err(|e| format!("{e:?}"))?;
    add(&mut b_build, 5)?;
    let b = b_build
        .seal(id(25)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = pages(sink.clone())?;
    let live_ref = register(&pages, &a, "live")?;
    let expired_ref = register(&pages, &b, "expired")?;
    let weak_b = Arc::downgrade(&b);
    drop(b);
    let mut next_build = sink
        .open_forest_in_lineage(id(26)?, vec![root()], b_lineage)
        .map_err(|e| format!("{e:?}"))?;
    add(&mut next_build, 9)?;
    let next = next_build
        .seal(id(27)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(weak_b.upgrade().is_none());
    let next_ref = register(&pages, &next, "new")?;
    assert!(pages.rows(&request(&live_ref, &a)).is_ok());
    assert!(pages.rows(&request(&next_ref, &next)).is_ok());
    assert_eq!(
        pages.rows(&request(&expired_ref, &next)).err(),
        Some(Error::ModelExpired)
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

#[test]
fn group_rows_keep_order_and_producer_evidence_and_reject_other_model_group() -> Result<(), String>
{
    use du_analysis_index::visualization::live_models::{GroupRows, SizeRange};
    use std::collections::HashMap;
    let sink = IngestSink::default();
    let mut b = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    add(&mut b, 5)?;
    let a = b
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let pages = pages(sink.clone())?;
    let mut first = make_row(&a)?;
    first.display_name = "first".into();
    let mut second = first.clone();
    second.display_name = "second".into();
    let mut total = first.size.clone();
    total.amount = SizeAmount::LowerBound {
        bytes: 99_u64.into(),
    };
    let range = SizeRange::Known {
        min: first.size.clone(),
        max: total.clone(),
    };
    let group = GroupRows {
        rows: vec![first.clone(), second.clone()],
        total: total.clone(),
        size_range: range.clone(),
    };
    let groups = HashMap::from([("group-a".into(), group)]);
    let model = pages
        .register(ModelInput {
            version: a.clone(),
            condition_fingerprint: "a".into(),
            rows: vec![],
            groups,
        })
        .map_err(|e| format!("{e:?}"))?;
    let other = pages
        .register(ModelInput {
            version: a.clone(),
            condition_fingerprint: "b".into(),
            rows: vec![],
            groups: HashMap::from([(
                "group-b".into(),
                GroupRows {
                    rows: vec![],
                    total: total.clone(),
                    size_range: SizeRange::Unknown,
                },
            )]),
        })
        .map_err(|e| format!("{e:?}"))?;
    let page = pages
        .group_rows(&model, "group-a", 1, 1)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(page.rows, vec![second]);
    assert_eq!(page.total_count, 2);
    assert_eq!(page.group_total, total);
    assert_eq!(page.size_range, range);
    assert_eq!(page.result_revision, *a.version_id());
    let mut checkpoints = 0;
    let cancelled = pages.group_rows_cancellable(&model, "group-a", 0, 2, &mut || {
        checkpoints += 1;
        checkpoints == 3
    });
    assert_eq!(cancelled.err(), Some(Error::Cancelled));
    let retry = pages
        .group_rows(&model, "group-a", 0, 2)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(retry.rows.len(), 2);
    assert_eq!(retry.group_total, total);
    assert_eq!(
        pages.group_rows(&other, "group-a", 0, 1).err(),
        Some(Error::InvalidGroup)
    );
    assert_eq!(
        pages.group_rows(&model, "group-a", 3, 1).err(),
        Some(Error::InvalidRange)
    );
    assert_eq!(
        pages.group_rows(&model, "group-a", 0, 1001).err(),
        Some(Error::InvalidRange)
    );
    assert!(
        pages
            .group_rows(&model, "group-a", 2, 1)
            .map_err(|e| format!("{e:?}"))?
            .rows
            .is_empty()
    );
    assert_eq!(
        pages
            .group_rows(&other, "group-b", 0, 1)
            .map_err(|e| format!("{e:?}"))?
            .size_range,
        SizeRange::Unknown
    );
    let mut replacing = sink.open(id(8)?).map_err(|e| format!("{e:?}"))?;
    add(&mut replacing, 9)?;
    let _partial = replacing
        .publish_partial(id(9)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        pages.group_rows(&model, "group-a", 0, 1).err(),
        Some(Error::StaleVersion)
    );
    replacing.abort();
    assert!(pages.group_rows(&model, "group-a", 0, 1).is_ok());
    pages.release_model(&model).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        pages.group_rows(&model, "group-a", 0, 1).err(),
        Some(Error::ModelExpired)
    );
    Ok(())
}
