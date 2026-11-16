#![cfg(target_os = "macos")]
use du_core_contracts::scan_observation::{
    AnchoredDirectory, DirectoryObservations, FilterDecision, FilteredBatch, MetadataObservation,
    ObservedEntry, ObservedKind, OpenedScopeRoot, ScopeRootPort,
};
use du_platform_os::macos_observation::MacScopeRoots;
use std::{
    fs,
    os::unix::{ffi::OsStrExt, fs::symlink},
    path::{Path, PathBuf},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = fs::canonicalize(directory.path())?;
    Ok((directory, path))
}

fn open(path: &Path) -> Result<OpenedScopeRoot, String> {
    MacScopeRoots
        .open_root(path.as_os_str().as_bytes())
        .map_err(|e| format!("{e:?}"))
}

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

fn find<'a>(entries: &'a [ObservedEntry], name: &[u8]) -> Result<&'a ObservedEntry, String> {
    entries
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| format!("missing {}", String::from_utf8_lossy(name)))
}

fn meta(entry: &ObservedEntry) -> Result<MetadataObservation, String> {
    entry.metadata.clone().map_err(|e| format!("{e:?}"))
}

fn link_creation_is_unavailable(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
    )
}

#[test]
fn filters_names_from_a_real_directory() -> TestResult {
    let (_guard, path) = fixture()?;
    for name in ["keep-a", "drop-a", "keep-b", "drop-b"] {
        fs::write(path.join(name), b"fixture")?;
    }
    let mut root = open(&path)?;
    let mut include = |name: &[u8]| {
        if name.starts_with(b"keep-") {
            FilterDecision::Admit
        } else {
            FilterDecision::Exclude
        }
    };
    let mut kept = Vec::new();
    loop {
        match root
            .directory
            .next_filtered(&mut include)
            .map_err(|error| format!("{error:?}"))?
        {
            FilteredBatch::Entries(entries) => {
                kept.extend(entries.into_iter().map(|entry| entry.name));
            }
            FilteredBatch::Progress => {}
            FilteredBatch::End => break,
            FilteredBatch::Undecidable => return Err("fixture filter became undecidable".into()),
        }
    }
    kept.sort();
    assert_eq!(kept, [b"keep-a".to_vec(), b"keep-b".to_vec()]);
    Ok(())
}

#[test]
fn reports_a_link_without_opening_its_target() -> TestResult {
    let (_guard, path) = fixture()?;
    let scope = path.join("scope");
    let target = path.join("target");
    fs::create_dir(&scope)?;
    fs::create_dir(&target)?;
    fs::write(target.join("target-only"), vec![7u8; 4096])?;
    let link_path = scope.join("to-target");
    if let Err(error) = symlink("../target", &link_path) {
        if link_creation_is_unavailable(&error) {
            eprintln!("skipped macOS symbolic-link case after creation failed: {error}");
            return Ok(());
        }
        return Err(error.into());
    }

    let mut root = open(&scope)?;
    let entries = all(&mut *root.directory)?;
    assert_eq!(entries.len(), 1);
    let link = find(&entries, b"to-target")?;
    assert_eq!(link.enumerated_kind, ObservedKind::Link);
    let observed = meta(link)?;
    assert_eq!(observed.kind, ObservedKind::Link);
    if let Ok(size) = &observed.size {
        assert_eq!(*size, fs::symlink_metadata(&link_path)?.len());
    }
    assert!(root.directory.open_child(link).is_err());
    assert_eq!(fs::read(target.join("target-only"))?.len(), 4096);
    Ok(())
}
