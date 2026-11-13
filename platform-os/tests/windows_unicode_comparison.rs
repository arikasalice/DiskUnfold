#![cfg(windows)]
//! Isolated NTFS observations; these examples do not define a path comparator.

use du_core_contracts::scan_observation::DirectoryObservations;
use du_platform_os::windows_observation::{case_sensitivity::DirectoryCaseMode, open_verified};
use std::{fs, fs::OpenOptions, path::Path, process::Command};

fn raw(name: &str) -> Vec<u8> {
    name.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn create(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    OpenOptions::new().write(true).create_new(true).open(path)?;
    Ok(())
}

fn unicode_pair(
    path: &Path,
    expected_mode: DirectoryCaseMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let names = ["\u{00e9}.txt", "e\u{0301}.txt"];
    for name in names {
        create(&path.join(name))?;
    }
    create(&path.join("Mixed.txt"))?;
    let mut directory = open_verified(path).map_err(|e| format!("{e:?}"))?;
    let evidence = directory.case_sensitivity().map_err(|e| format!("{e:?}"))?;
    assert_eq!(evidence.mode, expected_mode);
    let handle_identity = directory.identity().map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (evidence.volume_serial, evidence.file_index),
        (handle_identity.volume, handle_identity.file_id)
    );
    let entries = directory.next_batch().map_err(|e| format!("{e:?}"))?;
    let first = entries
        .iter()
        .find(|entry| entry.name == raw(names[0]))
        .ok_or("missing composed name")?;
    let second = entries
        .iter()
        .find(|entry| entry.name == raw(names[1]))
        .ok_or("missing decomposed name")?;
    assert_ne!(first.name, second.name);
    assert_ne!(first.enumerated_inode, 0);
    assert_ne!(second.enumerated_inode, 0);
    assert_ne!(first.enumerated_inode, second.enumerated_inode);
    for item in [first, second] {
        assert_eq!(
            item.enumerated_inode,
            item.metadata.as_ref().map_err(|e| format!("{e:?}"))?.inode
        );
    }
    assert_eq!(
        path.join("mixed.txt").exists(),
        expected_mode == DirectoryCaseMode::Insensitive
    );
    eprintln!(
        "NTFS fixture: {expected_mode:?}; canonical-equivalent UTF-16 spellings retained as distinct names and file IDs"
    );
    Ok(())
}

#[test]
fn unicode_spellings_in_ordinary_directory() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    unicode_pair(temp.path(), DirectoryCaseMode::Insensitive)
}

#[test]
#[ignore = "requires permission to enable NTFS per-directory case sensitivity"]
fn unicode_spellings_in_sensitive_directory() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let sensitive = temp.path().join("sensitive");
    fs::create_dir(&sensitive)?;
    let result = Command::new("fsutil.exe")
        .args(["file", "setCaseSensitiveInfo"])
        .arg(&sensitive)
        .arg("enable")
        .output()?;
    if !result.status.success() {
        return Err(format!(
            "case-sensitive fixture unavailable (fsutil exit {:?}): {} {}",
            result.status.code(),
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    unicode_pair(&sensitive, DirectoryCaseMode::Sensitive)
}

#[test]
fn same_name_replacement_does_not_preserve_file_identity() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("item");
    create(&path)?;
    let mut first_directory = open_verified(temp.path()).map_err(|e| format!("{e:?}"))?;
    let first = first_directory
        .next_batch()
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .find(|x| x.name == raw("item"))
        .ok_or("missing original")?;
    fs::rename(&path, temp.path().join("old-item"))?;
    create(&path)?;
    let mut second_directory = open_verified(temp.path()).map_err(|e| format!("{e:?}"))?;
    let second = second_directory
        .next_batch()
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .find(|x| x.name == raw("item"))
        .ok_or("missing replacement")?;
    assert_eq!(first.name, second.name);
    assert_ne!(first.enumerated_inode, second.enumerated_inode);
    assert_eq!(
        first.enumerated_inode,
        first.metadata.map_err(|e| format!("{e:?}"))?.inode
    );
    assert_eq!(
        second.enumerated_inode,
        second.metadata.map_err(|e| format!("{e:?}"))?.inode
    );
    Ok(())
}
