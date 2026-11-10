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

#[test]
fn timed_filter_records_only_admitted_successful_observations()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::{Timestamp, scan_observation::TimedFilteredBatch};
    use std::cell::Cell;
    let fixture = tempfile::tempdir()?;
    std::fs::write(fixture.path().join("allowed"), b"ok")?;
    std::fs::write(fixture.path().join("excluded"), b"no")?;
    let excluded: Vec<u8> = "excluded"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let ticks = Cell::new(0);
    let time = Timestamp::from_unix_millis(1234).ok_or("invalid time")?;
    let mut root = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    PROBES.with(|value| value.set(0));
    let mut items = Vec::new();
    loop {
        match root
            .next_filtered_timed(
                &mut |name| {
                    if name == excluded {
                        FilterDecision::Exclude
                    } else {
                        FilterDecision::Admit
                    }
                },
                &|| {
                    ticks.set(ticks.get() + 1);
                    Some(time.clone())
                },
            )
            .map_err(|e| format!("{e:?}"))?
        {
            TimedFilteredBatch::Entries(entries) => items.extend(entries),
            TimedFilteredBatch::Progress => {}
            TimedFilteredBatch::End => break,
            TimedFilteredBatch::Undecidable => return Err("unexpected undecidable".into()),
        }
    }
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].observed_at.as_ref(), Some(&time));
    assert_eq!(ticks.get(), 1);
    PROBES.with(|value| assert_eq!(value.get(), 1));
    Ok(())
}

#[test]
#[allow(unsafe_code)]
fn timed_filter_does_not_timestamp_offline_failures() -> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::scan_observation::TimedFilteredBatch;
    use std::{cell::Cell, os::windows::ffi::OsStrExt};
    use windows::{
        Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW,
        },
        core::PCWSTR,
    };
    let fixture = tempfile::tempdir()?;
    let path = fixture.path().join("offline");
    std::fs::write(&path, b"x")?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the isolated fixture path is NUL-terminated and remains valid.
    unsafe {
        SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_OFFLINE)?;
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut root = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
        let ticks = Cell::new(0);
        let batch = root
            .next_filtered_timed(&mut |_| FilterDecision::Admit, &|| {
                ticks.set(ticks.get() + 1);
                None
            })
            .map_err(|e| format!("{e:?}"))?;
        let TimedFilteredBatch::Entries(items) = batch else {
            return Err("missing batch".into());
        };
        assert_eq!(items.len(), 1);
        assert!(items[0].entry.metadata.is_err());
        assert!(items[0].observed_at.is_none());
        assert_eq!(ticks.get(), 0);
        Ok(())
    })();
    // SAFETY: restore the same isolated fixture for cleanup.
    unsafe {
        SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_NORMAL)?;
    }
    result
}

#[test]
fn timed_root_reads_live_handle_before_clock() -> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::{Timestamp, scan_observation::AnchoredDirectory};
    use std::cell::Cell;
    let fixture = tempfile::tempdir()?;
    let root = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    let calls = Cell::new(0);
    let expected = Timestamp::from_unix_millis(1234).ok_or("invalid time")?;
    let (modified, observed) = root.observed_modified_at_timed(&|| {
        calls.set(calls.get() + 1);
        Some(expected.clone())
    });
    assert!(modified.is_some());
    assert_eq!(observed, Some(expected));
    assert_eq!(calls.get(), 1);
    Ok(())
}
