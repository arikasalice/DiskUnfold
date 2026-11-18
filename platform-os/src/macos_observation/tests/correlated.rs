use super::super::MacDirectory;
use super::TestResult;
use du_core_contracts::scan_observation::{DirectoryObservations, FilterDecision};
use std::{
    os::{fd::AsFd, unix::fs::MetadataExt},
    sync::Arc,
};

#[test]
fn correlated_stream_errors_are_terminal_and_cannot_fall_back() -> TestResult {
    let file = tempfile::tempfile()?;
    let metadata = file.metadata()?;
    let fd = file.as_fd().try_clone_to_owned()?;
    let mut directory = MacDirectory::new(
        fd,
        (metadata.dev(), metadata.ino()),
        Arc::from(&b"uuid:test"[..]),
        None,
    );
    directory
        .begin_correlated_filtered()
        .map_err(|error| format!("{error:?}"))?;
    let first = directory
        .next_correlated_event(&mut |_| FilterDecision::Admit, &|| None)
        .err()
        .ok_or("a non-directory descriptor produced an event")?;
    let second = directory
        .next_correlated_event(&mut |_| FilterDecision::Admit, &|| None)
        .err()
        .ok_or("a stream error was followed by another event")?;
    assert_eq!(first, second);

    assert!(directory.next_batch().is_err());
    assert!(
        directory
            .next_filtered(&mut |_| FilterDecision::Admit)
            .is_err()
    );
    assert!(directory.begin_correlated_filtered().is_err());
    Ok(())
}
