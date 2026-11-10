use du_analysis_index::{BuildHandle, IngestSink, QueryError};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};
use std::sync::Arc;

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn build(sink: &IngestSink, lineage: &Id, scope: u32) -> Result<BuildHandle, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&[67, 0]),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_forest_in_lineage(id(scope)?, vec![root], lineage.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    Ok(build)
}
fn current(sink: &IngestSink) -> Result<Option<Id>, String> {
    Ok(sink
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .map(|r| r.version_id().clone()))
}

#[test]
fn same_scope_reuses_lineage_and_independent_currents_survive_replacement() -> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("scope-a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("scope-b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        a,
        sink.bind_current_analysis("scope-a", id(3)?)
            .map_err(|e| format!("{e:?}"))?
    );
    let old = build(&sink, &a, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &b, 11)?
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let new = build(&sink, &a, 12)?
        .seal(id(22)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(old.lineage_id(), Some(&a));
    assert_eq!(new.lineage_id(), Some(&a));
    assert_eq!(
        sink.pin_current(&id(20)?).err(),
        Some(QueryError::StaleVersion)
    );
    assert_eq!(old.items().len(), 1);
    assert_eq!(
        sink.pin_current(&id(21)?)
            .map_err(|e| format!("{e:?}"))?
            .lineage_id(),
        Some(&b)
    );
    assert_eq!(current(&sink)?, Some(id(22)?));
    assert!(!Arc::ptr_eq(&old, &new));
    Ok(())
}

#[test]
fn failure_restores_own_baseline_and_previous_focus_without_cross_lineage_replacement()
-> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &a, 10)?
        .seal(id(20)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &b, 11)?
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let mut failed = build(&sink, &a, 12)?;
    let pin = failed
        .publish_partial(id(22)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(failed);
    assert_eq!(current(&sink)?, Some(id(21)?));
    assert!(sink.pin_current(&id(20)?).is_ok());
    assert!(sink.pin_current(&id(21)?).is_ok());
    assert_eq!(
        sink.pin_current(&id(22)?).err(),
        Some(QueryError::StaleVersion)
    );
    assert_eq!(pin.items().len(), 1);
    let c = sink
        .bind_current_analysis("c", id(3)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut failed = build(&sink, &c, 13)?;
    failed
        .publish_partial(id(23)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(failed);
    assert_eq!(current(&sink)?, Some(id(21)?));
    assert!(sink.pin_current(&id(20)?).is_ok());
    Ok(())
}

#[test]
fn abort_only_reverts_focus_when_its_build_last_changed_it() -> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut a_build = build(&sink, &a, 10)?;
    a_build
        .publish_partial(id(20)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut b_build = build(&sink, &b, 11)?;
    b_build
        .publish_partial(id(21)?)
        .map_err(|e| format!("{e:?}"))?;
    a_build
        .publish_partial(id(22)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(a_build);
    assert_eq!(current(&sink)?, Some(id(21)?));
    drop(b_build);
    assert_eq!(current(&sink)?, None);
    Ok(())
}

#[test]
fn first_failed_build_with_no_baseline_leaves_no_current_result() -> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut failed = build(&sink, &a, 10)?;
    let partial = failed
        .publish_partial(id(20)?)
        .map_err(|e| format!("{e:?}"))?;
    drop(failed);
    assert_eq!(current(&sink)?, None);
    assert_eq!(sink.pin_current(&id(20)?).err(), Some(QueryError::NotFound));
    assert_eq!(partial.items().len(), 1);
    Ok(())
}

#[test]
fn failed_build_does_not_steal_focus_from_a_later_other_lineage() -> Result<(), String> {
    let sink = IngestSink::default();
    let a = sink
        .bind_current_analysis("a", id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let b = sink
        .bind_current_analysis("b", id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut failed = build(&sink, &a, 10)?;
    failed
        .publish_partial(id(20)?)
        .map_err(|e| format!("{e:?}"))?;
    build(&sink, &b, 11)?
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    drop(failed);
    assert_eq!(current(&sink)?, Some(id(21)?));
    assert!(sink.pin_current(&id(21)?).is_ok());
    Ok(())
}
