#![cfg(windows)]
use du_core_contracts::scan_observation::DirectoryObservations;
use du_platform_os::windows_observation::{case_sensitivity::DirectoryCaseMode, open_verified};
use std::{fs, process::Command};

#[test]
fn reports_flag_for_the_open_directory_without_reopening_its_name()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("original");
    fs::create_dir(&path)?;
    fs::write(path.join("MiXeD"), b"fixture")?;
    let directory = open_verified(&path).map_err(|e| format!("{e:?}"))?;
    let first = directory.case_sensitivity().map_err(|e| format!("{e:?}"))?;
    let id = directory.identity().map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.file_index, id.file_id);
    assert_eq!(first.volume_serial, id.volume);
    assert_eq!(
        first.mode == DirectoryCaseMode::Insensitive,
        path.join("mixed").exists()
    );
    fs::rename(&path, temp.path().join("renamed"))?;
    fs::create_dir(&path)?;
    let after = directory.case_sensitivity().map_err(|e| format!("{e:?}"))?;
    assert_eq!(first, after);
    assert_ne!(
        first.file_index,
        open_verified(&path)
            .map_err(|e| format!("{e:?}"))?
            .identity()
            .map_err(|e| format!("{e:?}"))?
            .file_id
    );
    Ok(())
}

#[test]
#[ignore = "requires permission to enable NTFS per-directory case sensitivity"]
fn directory_flags_are_not_inferred_from_the_parent() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let child = temp.path().join("case-sensitive");
    fs::create_dir(&child)?;
    let mut child_directory = open_verified(&child).map_err(|e| format!("{e:?}"))?;
    let initial = child_directory
        .case_sensitivity()
        .map_err(|e| format!("{e:?}"))?;
    let set = Command::new("fsutil.exe")
        .args(["file", "setCaseSensitiveInfo"])
        .arg(&child)
        .arg("enable")
        .output()?;
    if !set.status.success() {
        return Err(format!(
            "case-sensitive fixture unavailable (fsutil exit {:?}): {} {}",
            set.status.code(),
            String::from_utf8_lossy(&set.stdout),
            String::from_utf8_lossy(&set.stderr)
        )
        .into());
    }
    let child_flag = child_directory
        .case_sensitivity()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(child_flag.mode, DirectoryCaseMode::Sensitive);
    assert_eq!(initial.file_index, child_flag.file_index);
    fs::write(child.join("Name"), b"a")?;
    fs::write(child.join("name"), b"b")?;
    assert_eq!(fs::read(child.join("Name"))?, b"a");
    assert_eq!(fs::read(child.join("name"))?, b"b");
    let observed = child_directory.next_batch().map_err(|e| format!("{e:?}"))?;
    for literal in ["Name", "name"] {
        let raw: Vec<u8> = literal.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let entry = observed
            .iter()
            .find(|entry| entry.name == raw)
            .ok_or("raw name missing")?;
        let metadata = entry.metadata.as_ref().map_err(|e| format!("{e:?}"))?;
        assert_ne!(entry.enumerated_inode, 0);
        assert_eq!(entry.enumerated_inode, metadata.inode);
        eprintln!(
            "{literal}: enumerated index {}, observed index {:?}",
            entry.enumerated_inode,
            entry.metadata.as_ref().ok().map(|meta| meta.inode)
        );
    }
    let mut scan_parent = open_verified(temp.path()).map_err(|e| format!("{e:?}"))?;
    let parent = scan_parent
        .case_sensitivity()
        .map_err(|e| format!("{e:?}"))?;
    let child_name: Vec<u8> = "case-sensitive"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let entry = scan_parent
        .next_batch()
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .find(|entry| entry.name == child_name)
        .ok_or("child missing")?;
    let (_, observed) = scan_parent
        .open_child_with_case(&entry)
        .map_err(|e| format!("{e:?}"))?;
    let observed = observed.map_err(|e| format!("{e:?}"))?;
    assert_eq!(observed, child_flag);
    assert_eq!(observed.volume_serial, parent.volume_serial);
    eprintln!(
        "Parent mode: {:?}; child before: {:?}; child after: {:?}",
        parent.mode, initial.mode, child_flag.mode
    );
    Ok(())
}

#[test]
fn a_file_cannot_be_used_as_a_verified_directory() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let file = temp.path().join("file");
    fs::write(&file, b"fixture")?;
    assert!(open_verified(&file).is_err());
    Ok(())
}

#[test]
fn child_case_observation_uses_scan_handle_and_rejects_replaced_entry()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let child = temp.path().join("child");
    fs::create_dir(&child)?;
    let mut parent = open_verified(temp.path()).map_err(|e| format!("{e:?}"))?;
    let entry = parent
        .next_batch()
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .find(|entry| {
            entry.name
                == "child"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
        })
        .ok_or("child not enumerated")?;
    let parent_case = parent.case_sensitivity().map_err(|e| format!("{e:?}"))?;
    let (opened, observed) = parent
        .open_child_with_case(&entry)
        .map_err(|e| format!("{e:?}"))?;
    let observed = observed.map_err(|e| format!("{e:?}"))?;
    let identity = opened.identity().map_err(|e| format!("{e:?}"))?;
    assert_eq!(observed.file_index, identity.file_id);
    assert_eq!(observed.volume_serial, identity.volume);
    assert_eq!(observed.volume_serial, parent_case.volume_serial);
    drop(opened);
    fs::rename(&child, temp.path().join("old-child"))?;
    fs::create_dir(&child)?;
    assert!(parent.open_child_with_case(&entry).is_err());
    fs::remove_dir(&child)?;
    assert!(parent.open_child_with_case(&entry).is_err());
    Ok(())
}
