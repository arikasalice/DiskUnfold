use super::*;
use du_core_contracts::scan_observation::{
    CorrelatedDirectoryEvent, DirectoryObservations, FilterDecision,
};
use std::collections::{HashMap, HashSet};

#[test]
fn correlated_windows_stream_counts_records_across_variable_sized_replies()
-> Result<(), Box<dyn std::error::Error>> {
    const ENTRY_COUNT: usize = 300;
    // Long names make the aggregate records exceed one 64-KiB reply. Do not
    // assert how many records any individual OS call returns.
    let fixture = tempfile::tempdir()?;
    let names: HashSet<Vec<u8>> = (0..ENTRY_COUNT)
        .map(|index| format!("child-{index:04}-{}", "x".repeat(180)))
        .map(|name| name.encode_utf16().flat_map(u16::to_le_bytes).collect())
        .collect();
    for index in 0..ENTRY_COUNT {
        let name = format!("child-{index:04}-{}", "x".repeat(180));
        std::fs::write(fixture.path().join(name), b"value")?;
    }

    let mut directory = open_verified(fixture.path()).map_err(|error| format!("{error:?}"))?;
    directory
        .begin_correlated_filtered()
        .map_err(|error| format!("{error:?}"))?;
    let mut callbacks = HashMap::new();
    let mut observed = HashSet::new();
    let mut next_child_id = 0u64;
    let end_count = loop {
        match directory
            .next_correlated_event(&mut |_| FilterDecision::Admit, &|| None)
            .map_err(|error| format!("{error:?}"))?
        {
            CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                raw_name,
                decision,
                ..
            } => {
                assert_eq!(child_id, next_child_id);
                assert!(names.contains(&raw_name));
                assert_eq!(decision, FilterDecision::Admit);
                assert!(callbacks.insert(child_id, raw_name).is_none());
                next_child_id += 1;
            }
            CorrelatedDirectoryEvent::Observation { child_id, entry } => {
                let callback_name = callbacks
                    .remove(&child_id)
                    .ok_or("observation did not match its callback")?;
                assert_eq!(entry.entry.name, callback_name);
                assert!(entry.entry.metadata.is_ok());
                assert!(entry.observed_at.is_none());
                assert!(observed.insert(entry.entry.name));
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
    assert_eq!(observed, names);
    Ok(())
}
