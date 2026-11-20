//! #211, F03 FR6.HD1: a dataless file record reports a placeholder and no allocation,
//! from the flags of the same record and without opening the file.
use super::*;
use du_core_contracts::scan_observation::PlaceholderObservation;

fn record(object_type: u32, flags: Option<u32>) -> bulk::Record {
    bulk::Record {
        name: b"x".to_vec(),
        device: Some(7),
        object_type: Some(object_type),
        flags,
        file_id: Some(42),
        link_count: Some(1),
        allocated: Some(0),
        data_length: Some(5_000_000),
        ..Default::default()
    }
}

#[test]
fn a_dataless_file_is_a_placeholder_without_an_allocation() -> TestResult {
    let parent = parent(7)?;
    let cloud = record(1, Some(dataless::SF_DATALESS));
    assert_eq!(
        cloud.placeholder(),
        Some(PlaceholderObservation::MacDataless)
    );
    let meta = parent
        .observe(cloud)
        .metadata
        .map_err(|e| format!("{e:?}"))?;
    // The logical size, identity and link count stay; st_blocks is not a local occupancy.
    assert_eq!((meta.size, meta.allocated), (Ok(5_000_000), None));
    assert_eq!(meta.link_count, Some(1));
    assert!(meta.space_identity.is_some());
    Ok(())
}

#[test]
fn only_a_file_with_the_dataless_flag_is_a_placeholder() -> TestResult {
    let parent = parent(7)?;
    // UF_COMPRESSED and no flag: a local file keeps the reported allocation.
    for flags in [Some(0x20), Some(0), None] {
        let local = record(1, flags);
        assert_eq!(local.placeholder(), None, "{flags:?}");
        let meta = parent
            .observe(local)
            .metadata
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(meta.allocated, Some(0), "{flags:?}");
    }
    // A directory or a link with the flag is not a file placeholder.
    for object_type in [2, 5] {
        let other = record(object_type, Some(dataless::SF_DATALESS));
        assert_eq!(other.placeholder(), None, "{object_type}");
    }
    Ok(())
}
