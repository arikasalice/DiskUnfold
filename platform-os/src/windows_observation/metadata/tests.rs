use super::super::open_verified;
use super::{other_volume, verified_identity};
use du_core_contracts::scan_observation::{AnchoredDirectory, DirectoryObservations, ObservedKind};

#[test]
fn zero_index_or_other_volume_yields_no_root_identity() {
    assert_eq!(verified_identity(7, 7, 0, None), None);
    assert_eq!(verified_identity(7, 8, 42, None), None);
    let ids = [
        5_u64.to_le_bytes().to_vec(),
        vec![1; 16],
        6_u64.to_le_bytes().to_vec(),
    ];
    let identity = verified_identity(7, 7, 42, Some(&ids));
    assert_eq!(identity.as_ref().map(|value| value.inode), Some(42));
    // A 128-bit ancestor ID cannot be compared with the 64-bit index.
    assert_eq!(identity.map(|value| value.ancestors), Some(vec![5, 6]));
}

#[test]
fn other_volume_entry_carries_identity_without_size_or_times() {
    let observed = other_volume(ObservedKind::Directory, 9, 11);
    assert_eq!((observed.device, observed.inode), (9, 11));
    assert!(observed.size.is_err());
    assert!(observed.modified_at.is_none() && observed.native_modified_at.is_none());
    assert!(observed.allocated.is_none() && observed.link_count.is_none());
    assert!(observed.space_identity.is_none());
}

#[test]
fn root_identity_is_comparable_with_child_metadata_and_keeps_ancestors()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir(dir.path().join("child"))?;
    std::fs::write(dir.path().join("file"), b"ok")?;
    let mut root = open_verified(dir.path()).map_err(|e| format!("{e:?}"))?;
    let identity = root
        .observed_identity()
        .ok_or("root identity unavailable")?;
    let entries = root.next_batch().map_err(|e| format!("{e:?}"))?;
    assert_eq!(entries.len(), 2);
    for entry in &entries {
        let meta = entry.metadata.as_ref().map_err(|e| format!("{e:?}"))?;
        assert_eq!(meta.device, identity.device);
        assert_ne!(meta.inode, identity.inode);
        assert!(!identity.ancestors.contains(&meta.inode));
    }
    // A temporary directory lies below at least the drive root.
    assert!(!identity.ancestors.is_empty());
    assert!(!identity.ancestors.contains(&identity.inode));
    let child = entries
        .iter()
        .find(|entry| {
            entry
                .metadata
                .as_ref()
                .is_ok_and(|meta| meta.kind == ObservedKind::Directory)
        })
        .ok_or("child directory missing")?;
    let opened = root.open_child(child).map_err(|e| format!("{e:?}"))?;
    let child_identity = opened
        .observed_identity()
        .ok_or("child identity unavailable")?;
    assert_eq!(child_identity.device, identity.device);
    assert_eq!(
        Some(child_identity.inode),
        child.metadata.as_ref().ok().map(|meta| meta.inode)
    );
    assert!(child_identity.ancestors.is_empty());
    Ok(())
}

#[test]
fn root_ancestors_run_from_the_volume_root_down_to_the_parent()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let child = dir.path().join("parent").join("child");
    std::fs::create_dir_all(&child)?;
    let inode = |path: &std::path::Path| -> Result<u64, String> {
        let directory = open_verified(path).map_err(|e| format!("{e:?}"))?;
        Ok(directory
            .observed_identity()
            .ok_or("identity unavailable")?
            .inode)
    };
    // Every directory from the drive root down to the parent, in that order.
    let mut expected = child
        .ancestors()
        .skip(1)
        .map(inode)
        .collect::<Result<Vec<_>, _>>()?;
    expected.reverse();
    let identity = open_verified(&child)
        .map_err(|e| format!("{e:?}"))?
        .observed_identity()
        .ok_or("identity unavailable")?;
    assert_eq!(identity.ancestors, expected);
    assert_eq!(identity.ancestors.len(), child.ancestors().count() - 1);
    Ok(())
}

#[test]
fn correlated_metadata_rejects_a_file_id_that_differs_from_the_enumeration_record()
-> Result<(), String> {
    use super::super::{info, relative, wide};
    use du_core_contracts::ports::PortErrorKind;
    use du_core_contracts::scan_observation::DirectoryObservations;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;

    let fixture = tempfile::tempdir().map_err(|error| error.to_string())?;
    std::fs::write(fixture.path().join("entry"), b"fixture").map_err(|error| error.to_string())?;
    let mut directory = open_verified(fixture.path()).map_err(|error| format!("{error:?}"))?;
    let entry = directory
        .next_batch()
        .map_err(|error| format!("{error:?}"))?
        .into_iter()
        .next()
        .ok_or("entry was not enumerated")?;
    let name = wide(&entry.name).map_err(|error| format!("{error:?}"))?;
    let file =
        relative(&directory.handle, &name, false, false).map_err(|error| format!("{error:?}"))?;
    let opened_id = super::super::identity(&info(&file).map_err(|error| format!("{error:?}"))?);
    let mismatched = directory.observe_correlated(
        entry.name.clone(),
        opened_id ^ 1,
        FILE_ATTRIBUTE_NORMAL.0,
        0,
        Ok(true),
    );

    assert_eq!(entry.enumerated_inode, opened_id);
    assert_eq!(mismatched.name, entry.name);
    assert_eq!(mismatched.enumerated_inode, opened_id ^ 1);
    assert_eq!(
        mismatched
            .metadata
            .as_ref()
            .err()
            .map(|error| error.kind.clone()),
        Some(PortErrorKind::IdentityChanged)
    );
    Ok(())
}

#[test]
fn correlated_metadata_is_withheld_when_the_case_mode_is_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::ports::{PortError, PortErrorKind};
    use du_core_contracts::scan_observation::DirectoryObservations;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;

    let fixture = tempfile::tempdir()?;
    std::fs::write(fixture.path().join("entry"), b"fixture")?;
    let mut directory = open_verified(fixture.path()).map_err(|error| format!("{error:?}"))?;
    let entry = directory
        .next_batch()
        .map_err(|error| format!("{error:?}"))?
        .into_iter()
        .next()
        .ok_or("entry was not enumerated")?;
    let withheld = directory.observe_correlated(
        entry.name.clone(),
        entry.enumerated_inode,
        FILE_ATTRIBUTE_NORMAL.0,
        0,
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "os.query_failed".into(),
            os_code: None,
        }),
    );

    assert_eq!(withheld.name, entry.name);
    assert_eq!(withheld.enumerated_inode, entry.enumerated_inode);
    assert_eq!(
        withheld
            .metadata
            .as_ref()
            .err()
            .map(|error| error.kind.clone()),
        Some(PortErrorKind::Io)
    );
    Ok(())
}
