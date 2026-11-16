use super::*;
use du_core_contracts::{
    Timestamp,
    scan_observation::{
        CorrelatedDirectoryEvent, FilterCallbackPhase, FilterDecision,
    },
};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
};

#[test]
fn correlated_stream_joins_filter_and_observation_for_more_than_one_batch() -> TestResult {
    const ENTRY_COUNT: usize = 513;
    let temp = tempfile::tempdir()?;
    let expected: HashSet<Vec<u8>> = (0..ENTRY_COUNT)
        .map(|index| format!("child-{index:04}").into_bytes())
        .collect();
    for name in &expected {
        fs::write(temp.path().join(OsStr::from_bytes(name)), b"value")?;
    }

    let mut root = open(temp.path())?;
    root.directory.begin_correlated_filtered().map_err(fail)?;
    let observed_at = Timestamp::parse("2026-11-16T00:00:00.000Z")?;
    let mut callbacks = HashMap::new();
    let mut observations = HashSet::new();
    let mut next_child_id = 0;
    let mut include = |_: &[u8]| FilterDecision::Admit;
    let clock = || Some(observed_at.clone());

    let end_count = loop {
        match root
            .directory
            .next_correlated_event(&mut include, &clock)
            .map_err(fail)?
        {
            CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                raw_name,
                decision,
                phase,
            } => {
                assert_eq!(child_id, next_child_id);
                assert_eq!(decision, FilterDecision::Admit);
                assert_eq!(phase, FilterCallbackPhase::BeforeMetadata);
                assert!(expected.contains(&raw_name));
                assert!(callbacks.insert(child_id, raw_name).is_none());
                next_child_id += 1;
            }
            CorrelatedDirectoryEvent::Observation { child_id, entry } => {
                let callback_name = callbacks
                    .remove(&child_id)
                    .ok_or("observation did not match a prior callback")?;
                assert_eq!(entry.entry.name, callback_name);
                assert!(entry.entry.metadata.is_ok());
                assert_eq!(entry.observed_at, Some(observed_at.clone()));
                assert!(observations.insert(entry.entry.name));
            }
            CorrelatedDirectoryEvent::Progress => {}
            CorrelatedDirectoryEvent::End {
                child_count_exclusive,
            } => break child_count_exclusive,
        }
    };

    assert_eq!(next_child_id, ENTRY_COUNT as u64);
    assert_eq!(end_count, ENTRY_COUNT as u64);
    assert!(callbacks.is_empty());
    assert_eq!(observations, expected);
    assert!(
        root.directory
            .next_correlated_event(&mut include, &clock)
            .is_err()
    );
    Ok(())
}

#[test]
fn correlated_empty_directory_ends_only_after_a_real_read() -> TestResult {
    let temp = tempfile::tempdir()?;
    let mut root = open(temp.path())?;
    root.directory.begin_correlated_filtered().map_err(fail)?;
    let mut include = |_: &[u8]| FilterDecision::Admit;
    let clock = || None;

    assert!(matches!(
        root.directory
            .next_correlated_event(&mut include, &clock)
            .map_err(fail)?,
        CorrelatedDirectoryEvent::End {
            child_count_exclusive: 0
        }
    ));
    assert!(
        root.directory
            .next_correlated_event(&mut include, &clock)
            .is_err()
    );
    Ok(())
}

#[test]
fn correlated_exclusion_has_no_later_observation_for_its_child_id() -> TestResult {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("keep"), b"value")?;
    fs::write(temp.path().join("skip"), b"value")?;
    let mut root = open(temp.path())?;
    root.directory.begin_correlated_filtered().map_err(fail)?;
    let mut callbacks = HashMap::new();
    let mut observed = HashSet::new();
    let mut next_child_id = 0;
    let mut include = |name: &[u8]| {
        if name == b"skip" {
            FilterDecision::Exclude
        } else {
            FilterDecision::Admit
        }
    };
    let clock = || None;

    let end_count = loop {
        match root
            .directory
            .next_correlated_event(&mut include, &clock)
            .map_err(fail)?
        {
            CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                raw_name,
                decision,
                ..
            } => {
                assert_eq!(child_id, next_child_id);
                next_child_id += 1;
                if raw_name == b"skip" {
                    assert_eq!(decision, FilterDecision::Exclude);
                } else {
                    assert_eq!(raw_name, b"keep");
                    assert_eq!(decision, FilterDecision::Admit);
                    assert!(callbacks.insert(child_id, raw_name).is_none());
                }
            }
            CorrelatedDirectoryEvent::Observation { child_id, entry } => {
                let callback_name = callbacks
                    .remove(&child_id)
                    .ok_or("observation did not match an admitted callback")?;
                assert_eq!(entry.entry.name, callback_name);
                assert!(observed.insert(entry.entry.name));
            }
            CorrelatedDirectoryEvent::Progress => {}
            CorrelatedDirectoryEvent::End {
                child_count_exclusive,
            } => break child_count_exclusive,
        }
    };

    assert_eq!(next_child_id, 2);
    assert_eq!(end_count, 2);
    assert!(callbacks.is_empty());
    let expected_observed: HashSet<_> = [b"keep".to_vec()].into_iter().collect();
    assert_eq!(observed, expected_observed);
    Ok(())
}

#[test]
fn correlated_metadata_error_stays_bound_to_child_and_time_remains_unknown() -> TestResult {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("vanished");
    fs::write(&path, b"value")?;
    let mut root = open(temp.path())?;
    root.directory.begin_correlated_filtered().map_err(fail)?;
    let mut include = |name: &[u8]| {
        assert_eq!(name, b"vanished");
        fs::remove_file(&path).expect("remove after enumeration and before metadata");
        FilterDecision::Admit
    };
    let clock_calls = Cell::new(0);
    let clock = || {
        clock_calls.set(clock_calls.get() + 1);
        Timestamp::from_unix_millis(0)
    };

    let child_id = match root
        .directory
        .next_correlated_event(&mut include, &clock)
        .map_err(fail)?
    {
        CorrelatedDirectoryEvent::FilterCallback {
            child_id,
            decision: FilterDecision::Admit,
            phase: FilterCallbackPhase::BeforeMetadata,
            ..
        } => child_id,
        other => return Err(format!("unexpected first event: {other:?}").into()),
    };
    match root
        .directory
        .next_correlated_event(&mut include, &clock)
        .map_err(fail)?
    {
        CorrelatedDirectoryEvent::Observation {
            child_id: id,
            entry,
        } => {
            assert_eq!(id, child_id);
            assert!(entry.entry.metadata.is_err());
            assert_eq!(entry.observed_at, None);
        }
        other => return Err(format!("unexpected second event: {other:?}").into()),
    }
    assert_eq!(clock_calls.get(), 0);
    assert!(matches!(
        root.directory
            .next_correlated_event(&mut include, &clock)
            .map_err(fail)?,
        CorrelatedDirectoryEvent::End {
            child_count_exclusive: 1
        }
    ));
    Ok(())
}

#[test]
fn undecidable_callbacks_remain_child_scoped_and_are_not_end() -> TestResult {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("first"), b"1")?;
    fs::write(temp.path().join("second"), b"2")?;
    let mut root = open(temp.path())?;
    root.directory.begin_correlated_filtered().map_err(fail)?;
    let mut include = |_: &[u8]| FilterDecision::Undecidable;
    let clock = || None;
    let mut next_child_id = 0;
    let mut callback_count = 0;

    let end_count = loop {
        match root
            .directory
            .next_correlated_event(&mut include, &clock)
            .map_err(fail)?
        {
            CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                decision,
                phase,
                ..
            } => {
                assert_eq!(child_id, next_child_id);
                assert_eq!(decision, FilterDecision::Undecidable);
                assert_eq!(phase, FilterCallbackPhase::BeforeMetadata);
                next_child_id += 1;
                callback_count += 1;
            }
            CorrelatedDirectoryEvent::Observation { .. } => {
                return Err("undecidable filtering produced an observation".into());
            }
            CorrelatedDirectoryEvent::Progress => {}
            CorrelatedDirectoryEvent::End {
                child_count_exclusive,
            } => break child_count_exclusive,
        }
    };

    assert_eq!(callback_count, 2);
    assert_eq!(next_child_id, 2);
    assert_eq!(end_count, 2);
    Ok(())
}

#[test]
fn correlated_selection_cannot_resume_or_mix_with_legacy_enumeration() -> TestResult {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("child"), b"value")?;
    let mut root = open(temp.path())?;
    {
        root.directory.begin_correlated_filtered().map_err(fail)?;
    }
    assert!(root.directory.next_batch().is_err());
    assert!(root.directory.begin_correlated_filtered().is_err());
    Ok(())
}
