#![cfg(windows)]
//! Isolated evidence about frozen names and the Windows enumeration boundary.
use du_core_contracts::scan_observation::{DirectoryObservations, FilterDecision, FilteredBatch};
use du_platform_os::windows_observation::open_verified;
use std::fs;

fn native(value: &str) -> Vec<u8> {
    value.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[test]
fn existing_alternate_case_resolves_to_raw_enumerated_name()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    fs::create_dir(fixture.path().join("PrivateFolder"))?;
    if !fixture.path().join("privatefolder").exists() {
        eprintln!("Case-sensitive test directory; case-insensitive probe skipped");
        return Ok(());
    }
    fs::write(fixture.path().join("keep"), b"ok")?;
    let resolved = open_verified(&fixture.path().join("privatefolder"))
        .map_err(|e| format!("{e:?}"))?
        .identity()
        .map_err(|e| format!("{e:?}"))?;
    let leaf = resolved
        .physical_path
        .rsplit(|unit| *unit == b'\\' as u16)
        .next()
        .ok_or("missing leaf")?;
    let canonical: Vec<_> = leaf.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    assert_eq!(canonical, native("PrivateFolder"));
    let mut directory = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    let mut received = Vec::new();
    loop {
        match directory
            .next_filtered(&mut |raw| {
                if raw == canonical {
                    FilterDecision::Exclude
                } else {
                    FilterDecision::Admit
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
    assert_eq!(received[0].name, native("keep"));
    Ok(())
}

#[test]
fn absent_exclusion_can_appear_after_review() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let requested = fixture.path().join("absent");
    assert!(open_verified(&requested).is_err());
    let mut directory = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    fs::create_dir(fixture.path().join("ABSENT"))?;
    let mut saw = Vec::new();
    loop {
        match directory
            .next_filtered(&mut |_| FilterDecision::Admit)
            .map_err(|e| format!("{e:?}"))?
        {
            FilteredBatch::Entries(entries) => saw.extend(entries),
            FilteredBatch::Progress => {}
            FilteredBatch::End => break,
            FilteredBatch::Undecidable => return Err("unexpected undecidable".into()),
        }
    }
    assert_eq!(saw.len(), 1);
    assert_eq!(saw[0].name, native("ABSENT"));
    Ok(())
}

#[test]
fn canonical_name_snapshot_does_not_survive_case_only_rename()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let original = fixture.path().join("PrivateFolder");
    let renamed = fixture.path().join("PRIVATEFOLDER");
    fs::create_dir(&original)?;
    if !fixture.path().join("privatefolder").exists() {
        eprintln!("Case-sensitive test directory; case-insensitive probe skipped");
        return Ok(());
    }
    let before = open_verified(&fixture.path().join("privatefolder"))
        .map_err(|e| format!("{e:?}"))?
        .identity()
        .map_err(|e| format!("{e:?}"))?;
    let leaf = before
        .physical_path
        .rsplit(|unit| *unit == b'\\' as u16)
        .next()
        .ok_or("missing leaf")?;
    let frozen: Vec<_> = leaf.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    let mut directory = open_verified(fixture.path()).map_err(|e| format!("{e:?}"))?;
    fs::rename(&original, &renamed)?;
    let after = open_verified(&renamed)
        .map_err(|e| format!("{e:?}"))?
        .identity()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(before.file_id, after.file_id);
    let mut received = Vec::new();
    loop {
        match directory
            .next_filtered(&mut |raw| {
                if raw == frozen {
                    FilterDecision::Exclude
                } else {
                    FilterDecision::Admit
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
    assert_eq!(received[0].name, native("PRIVATEFOLDER"));
    assert_ne!(received[0].name, frozen);
    Ok(())
}

#[test]
#[allow(unsafe_code)]
fn short_alias_and_long_name_resolve_to_same_directory_when_available()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows::{Win32::Storage::FileSystem::GetShortPathNameW, core::PCWSTR};
    let fixture = tempfile::tempdir()?;
    let long = fixture.path().join("LongFolderNameForComparisonProbe");
    fs::create_dir(&long)?;
    let wide: Vec<u16> = long.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut result = vec![0u16; 32768];
    // SAFETY: the source is NUL-terminated and the destination is writable for its length.
    let length = unsafe { GetShortPathNameW(PCWSTR(wide.as_ptr()), Some(&mut result)) } as usize;
    if length == 0 || length >= result.len() {
        return Err("short-path query failed".into());
    }
    result.truncate(length);
    let short = std::path::PathBuf::from(std::ffi::OsString::from_wide(&result));
    if short.file_name() == long.file_name() {
        eprintln!("No distinct 8.3 name available on the test volume");
        return Ok(());
    }
    let long_id = open_verified(&long)
        .map_err(|e| format!("{e:?}"))?
        .identity()
        .map_err(|e| format!("{e:?}"))?;
    let short_id = open_verified(&short)
        .map_err(|e| format!("{e:?}"))?
        .identity()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(long_id.volume, short_id.volume);
    assert_eq!(long_id.file_id, short_id.file_id);
    assert_eq!(long_id.physical_path, short_id.physical_path);
    Ok(())
}
