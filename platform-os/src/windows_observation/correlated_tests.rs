use super::{PROBES, WindowsDirectory, open_verified};
use du_core_contracts::{
    Timestamp,
    scan_observation::{
        CorrelatedDirectoryEvent, DirectoryObservations, FilterCallbackPhase, FilterDecision,
        TimedObservedEntry,
    },
};
use std::{cell::Cell, collections::BTreeMap, fs};

fn native_name(name: &str) -> Vec<u8> {
    name.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

struct CorrelatedEvents {
    callbacks: BTreeMap<u64, (Vec<u8>, FilterDecision, FilterCallbackPhase)>,
    observations: BTreeMap<u64, TimedObservedEntry>,
    child_count_exclusive: u64,
}

fn collect_correlated_events(
    directory: &mut WindowsDirectory,
    decisions: &[(Vec<u8>, FilterDecision)],
    timestamp: &Timestamp,
    clock_calls: &Cell<usize>,
) -> Result<CorrelatedEvents, Box<dyn std::error::Error>> {
    let mut callbacks = BTreeMap::new();
    let mut observations = BTreeMap::new();
    let child_count_exclusive = loop {
        let mut include = |name: &[u8]| {
            decisions
                .iter()
                .find(|(candidate, _)| candidate.as_slice() == name)
                .map(|(_, decision)| *decision)
                .unwrap_or(FilterDecision::Undecidable)
        };
        let clock = || {
            clock_calls.set(clock_calls.get() + 1);
            Some(timestamp.clone())
        };
        match directory
            .next_correlated_event(&mut include, &clock)
            .map_err(|error| format!("{error:?}"))?
        {
            CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                raw_name,
                decision,
                phase,
            } => {
                assert!(
                    callbacks
                        .insert(child_id, (raw_name, decision, phase))
                        .is_none()
                );
            }
            CorrelatedDirectoryEvent::Observation { child_id, entry } => {
                assert!(callbacks.contains_key(&child_id));
                assert!(observations.insert(child_id, entry).is_none());
            }
            CorrelatedDirectoryEvent::Progress => {}
            CorrelatedDirectoryEvent::End {
                child_count_exclusive,
            } => break child_count_exclusive,
        }
    };

    Ok(CorrelatedEvents {
        callbacks,
        observations,
        child_count_exclusive,
    })
}

#[test]
fn correlated_end_counts_all_dense_child_ids_and_keeps_only_admitted_observations()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    for name in ["admit", "exclude", "undecidable"] {
        fs::write(fixture.path().join(name), b"fixture")?;
    }
    let decisions = [
        (native_name("admit"), FilterDecision::Admit),
        (native_name("exclude"), FilterDecision::Exclude),
        (native_name("undecidable"), FilterDecision::Undecidable),
    ];
    let timestamp = Timestamp::from_unix_millis(1234).ok_or("invalid test timestamp")?;
    let clock_calls = Cell::new(0usize);
    let mut directory = open_verified(fixture.path()).map_err(|error| format!("{error:?}"))?;
    directory
        .begin_correlated_filtered()
        .map_err(|error| format!("{error:?}"))?;

    PROBES.with(|count| count.set(0));
    let CorrelatedEvents {
        callbacks,
        observations,
        child_count_exclusive,
    } = collect_correlated_events(&mut directory, &decisions, &timestamp, &clock_calls)?;

    assert_eq!(child_count_exclusive, 3);
    assert_eq!(callbacks.keys().copied().collect::<Vec<_>>(), vec![0, 1, 2]);
    assert_eq!(callbacks.len(), 3);
    for (&child_id, (raw_name, decision, phase)) in &callbacks {
        assert_eq!(*phase, FilterCallbackPhase::BeforeMetadata);
        match decision {
            FilterDecision::Admit => {
                let entry = observations
                    .get(&child_id)
                    .ok_or("admitted child missing observation")?;
                assert_eq!(entry.entry.name.as_slice(), raw_name.as_slice());
                assert!(entry.entry.metadata.is_ok());
                assert_eq!(entry.observed_at, Some(timestamp.clone()));
            }
            FilterDecision::Exclude | FilterDecision::Undecidable => {
                assert!(!observations.contains_key(&child_id));
            }
        }
        let expected = decisions
            .iter()
            .find(|(candidate, _)| candidate.as_slice() == raw_name.as_slice())
            .map(|(_, decision)| decision)
            .ok_or("unexpected callback name")?;
        assert_eq!(decision, expected);
    }
    assert_eq!(observations.len(), 1);
    assert_eq!(clock_calls.get(), 1);
    PROBES.with(|count| assert_eq!(count.get(), 1));

    assert!(directory.next_batch().is_err());
    assert!(
        directory
            .next_filtered(&mut |_| FilterDecision::Admit)
            .is_err()
    );
    assert!(directory.begin_correlated_filtered().is_err());
    Ok(())
}

#[test]
fn correlated_empty_directory_ends_with_zero_child_count() -> Result<(), Box<dyn std::error::Error>>
{
    const MAX_EMPTY_DIRECTORY_CALLS: usize = 8;

    let fixture = tempfile::tempdir()?;
    let mut directory = open_verified(fixture.path()).map_err(|error| format!("{error:?}"))?;
    directory
        .begin_correlated_filtered()
        .map_err(|error| format!("{error:?}"))?;
    let mut calls = 0usize;
    let mut progress_events = 0u64;
    let child_count_exclusive = loop {
        if calls == MAX_EMPTY_DIRECTORY_CALLS {
            return Err(format!(
                "empty directory did not reach End within {MAX_EMPTY_DIRECTORY_CALLS} calls; last event was Progress ({progress_events} Progress events)"
            )
            .into());
        }
        calls += 1;
        let event = directory
            .next_correlated_event(&mut |_| FilterDecision::Admit, &|| None)
            .map_err(|error| format!("empty directory enumeration failed before EOF: {error:?}"))?;
        match event {
            CorrelatedDirectoryEvent::Progress => {
                progress_events = progress_events.saturating_add(1);
            }
            CorrelatedDirectoryEvent::End {
                child_count_exclusive,
            } => break child_count_exclusive,
            event => {
                return Err(format!("empty directory emitted a child event: {event:?}").into());
            }
        }
    };
    assert_eq!(
        child_count_exclusive, 0,
        "empty directory ended after {progress_events} Progress events"
    );
    assert!(
        directory
            .next_correlated_event(&mut |_| FilterDecision::Admit, &|| None)
            .is_err()
    );
    Ok(())
}

#[test]
#[allow(unsafe_code)]
fn correlated_metadata_failure_is_preserved_without_a_synthetic_time()
-> Result<(), Box<dyn std::error::Error>> {
    use std::{cell::Cell, os::windows::ffi::OsStrExt};
    use windows::{
        Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW,
        },
        core::PCWSTR,
    };

    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("offline");
    fs::write(&path, b"fixture")?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the isolated fixture path is NUL-terminated and remains valid.
    unsafe {
        SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_OFFLINE)?;
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut directory = open_verified(fixture.path()).map_err(|error| format!("{error:?}"))?;
        directory
            .begin_correlated_filtered()
            .map_err(|error| format!("{error:?}"))?;
        let clock_calls = Cell::new(0usize);
        let mut saw_callback = false;
        let mut saw_observation = false;
        loop {
            match directory
                .next_correlated_event(&mut |_| FilterDecision::Admit, &|| {
                    clock_calls.set(clock_calls.get() + 1);
                    None
                })
                .map_err(|error| format!("{error:?}"))?
            {
                CorrelatedDirectoryEvent::FilterCallback {
                    child_id,
                    raw_name,
                    decision,
                    ..
                } => {
                    assert_eq!(child_id, 0);
                    assert_eq!(raw_name, native_name("offline"));
                    assert_eq!(decision, FilterDecision::Admit);
                    saw_callback = true;
                }
                CorrelatedDirectoryEvent::Observation { child_id, entry } => {
                    assert_eq!(child_id, 0);
                    assert!(entry.entry.metadata.is_err());
                    assert!(entry.observed_at.is_none());
                    assert!(entry.placeholder_observation.is_none());
                    saw_observation = true;
                }
                CorrelatedDirectoryEvent::Progress => {}
                CorrelatedDirectoryEvent::End {
                    child_count_exclusive,
                } => {
                    assert_eq!(child_count_exclusive, 1);
                    break;
                }
            }
        }
        assert!(saw_callback);
        assert!(saw_observation);
        assert_eq!(clock_calls.get(), 0);
        Ok(())
    })();
    // SAFETY: restore the same isolated fixture before cleanup.
    unsafe {
        SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_NORMAL)?;
    }
    result
}
