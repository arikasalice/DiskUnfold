#![cfg(target_os = "macos")]
//! Volumes in other formats than APFS (#43), on attached disk images.
//!
//! A FAT volume may not return a directory's length in the bulk reply; the
//! adapter must then report the size as unknown, never as zero.
mod disk_image;
mod machine;

use du_core_contracts::scan_observation::{
    DirectoryObservations, ObservedEntry, ObservedKind, ScopeRootPort,
};
use du_platform_os::macos_observation::MacScopeRoots;
use std::{
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn all(directory: &mut dyn DirectoryObservations) -> Result<Vec<ObservedEntry>, String> {
    let mut entries = Vec::new();
    loop {
        let batch = directory.next_batch().map_err(|e| format!("{e:?}"))?;
        if batch.is_empty() {
            return Ok(entries);
        }
        entries.extend(batch);
    }
}

#[test]
fn a_fat_directory_size_is_its_length_or_unknown_never_a_made_up_zero() -> TestResult {
    let work = tempfile::tempdir()?;
    let mount = fs::canonicalize(work.path())?.join("fat");
    let image = work.path().join("fat.dmg");
    let attached = match disk_image::attach(&image, &mount, "MS-DOS FAT32", "40m") {
        Ok(attached) => attached,
        Err(reason) => return machine::skip(&format!("no FAT image could be attached: {reason}")),
    };
    let scope = mount.join("scope");
    fs::create_dir_all(scope.join("sub"))?;
    fs::write(scope.join("sub").join("inner.bin"), vec![1u8; 300])?;
    fs::write(scope.join("top.bin"), vec![2u8; 77])?;
    let mut root = MacScopeRoots
        .open_root(scope.as_os_str().as_bytes())
        .map_err(|e| format!("{e:?}"))?;
    let entries = all(&mut *root.directory)?;
    let find = |name: &[u8]| {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or(format!("missing {}", String::from_utf8_lossy(name)))
    };
    let sub = find(b"sub")?;
    let sub_meta = sub.metadata.as_ref().map_err(|e| format!("{e:?}"))?;
    let length = fs::symlink_metadata(scope.join("sub"))?.size();
    eprintln!(
        "F02_MACOS_FAT directory size={:?} st_size={length} kind={:?}",
        sub_meta.size, sub_meta.kind
    );
    assert_eq!(sub_meta.kind, ObservedKind::Directory);
    match &sub_meta.size {
        Ok(size) => assert_eq!(*size, length, "a reported size is the length"),
        Err(error) => assert_eq!(error.reason_key, "os.unsupported", "{error:?}"),
    }
    let top = find(b"top.bin")?;
    let top_meta = top.metadata.as_ref().map_err(|e| format!("{e:?}"))?;
    assert_eq!(top_meta.kind, ObservedKind::File);
    assert_eq!(top_meta.size.as_ref().ok(), Some(&77));
    drop(root);
    drop(attached);
    Ok(())
}
