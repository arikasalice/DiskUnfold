#![cfg(windows)]
use du_core_contracts::scan_observation::{DirectoryObservations, PlaceholderObservation};
use du_platform_os::windows_observation::open_verified;
use std::fs;

#[test]
fn ordinary_file_reports_only_historical_recall_flag_observation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    fs::write(fixture.path().join("item"), b"fixture")?;
    let mut directory = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    let batch = directory
        .next_batch_timed(&|| None)
        .map_err(|e| format!("{e:?}"))?;
    let name: Vec<u8> = "item".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let item = batch
        .iter()
        .find(|item| item.entry.name == name)
        .ok_or("missing item")?;
    assert!(item.entry.metadata.is_ok());
    assert_eq!(
        item.placeholder_observation,
        Some(PlaceholderObservation::WindowsNoRecallDataFlags)
    );
    Ok(())
}
