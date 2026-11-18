use du_core_contracts::{
    Timestamp,
    ports::{PortError, PortErrorKind},
    scan_observation::{
        CorrelatedDirectoryEvent, DirectoryObservations, FilterCallbackPhase, FilterDecision,
        FilteredBatch, MetadataObservation, ObservedEntry, ObservedKind, TimedObservedEntry,
    },
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct LegacyDirectory {
    batch_calls: usize,
    filtered_calls: usize,
}

impl DirectoryObservations for LegacyDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.batch_calls += 1;
        Ok(Vec::new())
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        self.filtered_calls += 1;
        let decision = include(&[0xff, b'x']);
        if decision == FilterDecision::Admit {
            Ok(FilteredBatch::Entries(vec![ObservedEntry {
                name: vec![0xff, b'x'],
                enumerated_inode: 7,
                enumerated_kind: ObservedKind::Unknown,
                metadata: Err(PortError {
                    kind: PortErrorKind::PermissionDenied,
                    reason_key: "scan.metadata_denied".into(),
                    os_code: Some(13),
                }),
            }]))
        } else {
            Ok(FilteredBatch::Undecidable)
        }
    }
}

#[test]
fn legacy_port_keeps_filtered_compatibility_and_defaults_correlated_methods() -> TestResult {
    let mut directory = LegacyDirectory {
        batch_calls: 0,
        filtered_calls: 0,
    };
    {
        let port: &mut dyn DirectoryObservations = &mut directory;
        let error = match port.begin_correlated_filtered() {
            Ok(_) => return Err("legacy port unexpectedly supports correlated enumeration".into()),
            Err(error) => error,
        };
        assert_eq!(error.kind, PortErrorKind::Unsupported);
        assert_eq!(error.reason_key, "os.unsupported");
    }
    assert_eq!(directory.batch_calls, 0);
    assert_eq!(directory.filtered_calls, 0);

    let mut correlated_directory = LegacyDirectory {
        batch_calls: 0,
        filtered_calls: 0,
    };
    let callback_calls = std::cell::Cell::new(0);
    let clock_calls = std::cell::Cell::new(0);
    let error = match (&mut correlated_directory as &mut dyn DirectoryObservations)
        .next_correlated_event(
            &mut |_| {
                callback_calls.set(callback_calls.get() + 1);
                FilterDecision::Admit
            },
            &|| {
                clock_calls.set(clock_calls.get() + 1);
                None
            },
        ) {
        Ok(_) => return Err("legacy port unexpectedly returned a correlated event".into()),
        Err(error) => error,
    };
    assert_eq!(error.kind, PortErrorKind::Unsupported);
    assert_eq!(error.reason_key, "os.unsupported");
    assert_eq!(error.os_code, None);
    assert_eq!(callback_calls.get(), 0);
    assert_eq!(clock_calls.get(), 0);
    assert_eq!(correlated_directory.batch_calls, 0);
    assert_eq!(correlated_directory.filtered_calls, 0);

    let mut called = false;
    let batch = (&mut directory as &mut dyn DirectoryObservations)
        .next_filtered(&mut |raw_name| {
            called = true;
            assert_eq!(raw_name, [0xff, b'x']);
            FilterDecision::Admit
        })
        .map_err(|error| format!("{error:?}"))?;
    let FilteredBatch::Entries(entries) = batch else {
        return Err("legacy next_filtered result changed".into());
    };
    assert!(called);
    let entry = entries.first().ok_or("legacy entry was lost")?;
    assert_eq!(entry.name, [0xff, b'x']);
    let Err(metadata_error) = &entry.metadata else {
        return Err("legacy metadata error was lost".into());
    };
    assert_eq!(metadata_error.os_code, Some(13));
    assert_eq!(directory.batch_calls, 0);
    assert_eq!(directory.filtered_calls, 1);
    Ok(())
}

#[test]
fn correlated_events_keep_child_ids_raw_names_times_errors_and_eof_count() -> TestResult {
    let observed_at = Timestamp::parse("2026-11-16T00:00:00.123Z")
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let metadata_error = PortError {
        kind: PortErrorKind::PermissionDenied,
        reason_key: "scan.metadata_denied".into(),
        os_code: Some(13),
    };
    let filter_event = CorrelatedDirectoryEvent::FilterCallback {
        child_id: 0,
        raw_name: vec![0xff, 0],
        decision: FilterDecision::Admit,
        phase: FilterCallbackPhase::BeforeMetadata,
    };
    assert_eq!(
        filter_event,
        CorrelatedDirectoryEvent::FilterCallback {
            child_id: 0,
            raw_name: vec![0xff, 0],
            decision: FilterDecision::Admit,
            phase: FilterCallbackPhase::BeforeMetadata,
        }
    );
    let undecidable = CorrelatedDirectoryEvent::FilterCallback {
        child_id: 1,
        raw_name: vec![0xfe],
        decision: FilterDecision::Undecidable,
        phase: FilterCallbackPhase::Unknown,
    };
    let CorrelatedDirectoryEvent::FilterCallback {
        child_id, decision, ..
    } = undecidable
    else {
        return Err("undecidable callback lost its child ID".into());
    };
    assert_eq!(child_id, 1);
    assert_eq!(decision, FilterDecision::Undecidable);

    let metadata = MetadataObservation {
        kind: ObservedKind::Directory,
        device: 1,
        inode: 8,
        size: Err(metadata_error.clone()),
        modified_at: None,
        native_modified_at: None,
        allocated: Some(16),
        link_count: None,
        space_identity: None,
    };
    let observation = CorrelatedDirectoryEvent::Observation {
        child_id: 0,
        entry: TimedObservedEntry {
            entry: ObservedEntry {
                name: vec![0xff, 0],
                enumerated_inode: 8,
                enumerated_kind: ObservedKind::Directory,
                metadata: Ok(metadata),
            },
            observed_at: Some(observed_at.clone()),
            placeholder_observation: None,
        },
    };
    let CorrelatedDirectoryEvent::Observation { child_id, entry } = observation else {
        return Err("expected an observation event".into());
    };
    assert_eq!(child_id, 0);
    assert_eq!(entry.entry.name, [0xff, 0]);
    assert_eq!(entry.observed_at, Some(observed_at));
    let Ok(metadata) = entry.entry.metadata else {
        return Err("per-entry metadata was lost".into());
    };
    let Err(size_error) = metadata.size else {
        return Err("individual size error was lost".into());
    };
    assert_eq!(size_error, metadata_error);
    assert_eq!(metadata.allocated, Some(16));

    let end = CorrelatedDirectoryEvent::End {
        child_count_exclusive: 2,
    };
    let CorrelatedDirectoryEvent::End {
        child_count_exclusive,
    } = end
    else {
        return Err("EOF child count was lost".into());
    };
    assert_eq!(child_count_exclusive, 2);
    Ok(())
}
