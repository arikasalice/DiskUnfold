use super::{PROBES, open_verified};
use du_core_contracts::scan_observation::{DirectoryObservations, FilterDecision, FilteredBatch};

#[test]
fn native_filter_skips_directory_before_metadata_probe() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir(dir.path().join("excluded"))?;
    std::fs::write(dir.path().join("allowed"), b"ok")?;
    let excluded: Vec<u8> = "excluded"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let allowed: Vec<u8> = "allowed"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut directory = open_verified(dir.path()).map_err(|e| format!("{e:?}"))?;
    PROBES.with(|count| count.set(0));
    let mut received = Vec::new();
    loop {
        match directory
            .next_filtered(&mut |name| {
                if name != excluded {
                    FilterDecision::Admit
                } else {
                    FilterDecision::Exclude
                }
            })
            .map_err(|e| format!("{e:?}"))?
        {
            FilteredBatch::Entries(entries) => received.extend(entries),
            FilteredBatch::Progress => {}
            FilteredBatch::End => break,
            FilteredBatch::Undecidable => return Err("unexpected undecidable".into()),
        }
    }
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].name, allowed);
    assert!(received[0].metadata.is_ok());
    PROBES.with(|count| assert_eq!(count.get(), 1));
    Ok(())
}

#[test]
fn undecidable_is_terminal_and_never_probes_the_item() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir(dir.path().join("unknown"))?;
    let mut directory = open_verified(dir.path()).map_err(|e| format!("{e:?}"))?;
    PROBES.with(|count| count.set(0));
    assert!(matches!(
        directory.next_filtered(&mut |_| FilterDecision::Undecidable),
        Ok(FilteredBatch::Undecidable)
    ));
    assert!(matches!(
        directory.next_filtered(&mut |_| FilterDecision::Admit),
        Ok(FilteredBatch::Undecidable)
    ));
    PROBES.with(|count| assert_eq!(count.get(), 0));
    Ok(())
}
