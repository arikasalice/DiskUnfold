use du_analysis_index::IngestSink;
use du_core_contracts::{Id, Timestamp, result::ResultCompleteness};
fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
#[test]
fn item_time_is_optional_and_frozen_per_publication() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let first = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    let observed = Timestamp::from_unix_millis(10).ok_or("invalid time")?;
    build
        .set_item_observed_at(0, &[], observed.clone())
        .map_err(|e| format!("{e:?}"))?;
    let end = build
        .seal_at(
            id(3)?,
            ResultCompleteness::Complete,
            Timestamp::from_unix_millis(20),
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.item_observed_at(0), None);
    assert_eq!(end.item_observed_at(0), Some(&observed));
    assert_ne!(
        end.item_observed_at(0),
        Timestamp::from_unix_millis(20).as_ref()
    );
    Ok(())
}

#[test]
fn ranges_preserve_missing_times_and_pinned_versions() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(10)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![b"child".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let early = Timestamp::from_unix_millis(10).ok_or("invalid time")?;
    let late = Timestamp::from_unix_millis(30).ok_or("invalid time")?;
    build
        .set_item_observed_at(0, &[], late.clone())
        .map_err(|e| format!("{e:?}"))?;
    let partial = build
        .publish_partial(id(11)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(partial.scope_observation_range().min, Some(&late));
    assert!(!partial.scope_observation_range().complete);
    build
        .set_item_observed_at(0, &[b"child".to_vec()], early.clone())
        .map_err(|e| format!("{e:?}"))?;
    let sealed = build
        .seal_at(
            id(12)?,
            ResultCompleteness::Complete,
            Timestamp::from_unix_millis(99),
        )
        .map_err(|e| format!("{e:?}"))?;
    let root = sealed.observation_range(0).ok_or("missing root")?;
    assert_eq!(
        (root.min, root.max, root.complete),
        (Some(&early), Some(&late), true)
    );
    let child = sealed.observation_range(1).ok_or("missing child")?;
    assert_eq!(
        (child.min, child.max, child.complete),
        (Some(&early), Some(&early), true)
    );
    assert_eq!(sealed.scope_observation_range(), root);
    assert!(sealed.observation_range(2).is_none());
    assert!(!partial.scope_observation_range().complete);
    Ok(())
}

#[test]
fn missing_child_time_does_not_produce_complete_interval() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(20)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![b"child".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let time = Timestamp::from_unix_millis(42).ok_or("invalid time")?;
    build
        .set_item_observed_at(0, &[], time.clone())
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(21)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let range = result.scope_observation_range();
    assert_eq!(
        (range.min, range.max, range.complete),
        (Some(&time), Some(&time), false)
    );
    Ok(())
}

#[test]
fn inaccessible_scope_keeps_present_bounds_but_not_complete() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(30)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let time = Timestamp::from_unix_millis(42).ok_or("invalid time")?;
    build
        .set_item_observed_at(0, &[], time.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .record_gap_at(0, vec![], "scan.path_denied".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(31)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let range = result.scope_observation_range();
    assert_eq!(
        (range.min, range.max, range.complete),
        (Some(&time), Some(&time), false)
    );
    assert!(!result.observation_range(0).ok_or("missing root")?.complete);
    Ok(())
}

#[test]
fn observation_range_orders_equivalent_separator_spellings_by_time() -> Result<(), String> {
    assert!(Timestamp::parse("2026-01-01T00:00:00Z").is_err());
    assert!(Timestamp::parse("2026-01-01T00:00:00.1Z").is_err());
    let earlier = Timestamp::parse("2026-01-01t00:00:01.000Z")?;
    let later = Timestamp::parse("2026-01-01T00:00:02.000Z")?;
    let sink = IngestSink::default();
    let mut build = sink.open(id(40)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(vec![b"child".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_item_observed_at(0, &[], later.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_item_observed_at(0, &[b"child".to_vec()], earlier.clone())
        .map_err(|e| format!("{e:?}"))?;
    let result = build
        .seal(id(41)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let range = result.scope_observation_range();
    assert_eq!(
        (range.min, range.max, range.complete),
        (Some(&earlier), Some(&later), true)
    );
    Ok(())
}
