use super::super::open_verified;
use du_core_contracts::scan_observation::{AnchoredDirectory, DirectoryObservations, ObservedKind};

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
