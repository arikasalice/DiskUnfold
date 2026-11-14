#![cfg(target_os = "macos")]
//! Links, permissions, races and pre-metadata filtering in the macOS adapter.
use du_core_contracts::{
    ports::{PortError, PortErrorKind},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, FilterDecision, FilteredBatch, ObservedEntry,
        ObservedKind, OpenedScopeRoot, ScopeRootPort,
    },
};
use du_platform_os::macos_observation::MacScopeRoots;
use std::{
    collections::BTreeSet,
    fs::{self, Permissions},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt, symlink},
    },
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

fn child(
    parent: &dyn AnchoredDirectory,
    entry: &ObservedEntry,
) -> Result<Box<dyn AnchoredDirectory>, String> {
    parent.open_child(entry).map_err(|e| format!("{e:?}"))
}

fn refused(parent: &dyn AnchoredDirectory, entry: &ObservedEntry) -> Result<PortError, String> {
    parent
        .open_child(entry)
        .err()
        .ok_or_else(|| format!("{} was opened", String::from_utf8_lossy(&entry.name)))
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

fn find<'a>(entries: &'a [ObservedEntry], name: &str) -> Result<&'a ObservedEntry, String> {
    entries
        .iter()
        .find(|entry| entry.name == name.as_bytes())
        .ok_or_else(|| format!("missing {name}"))
}

fn kind(entry: &ObservedEntry) -> Option<ObservedKind> {
    entry.metadata.as_ref().ok().map(|meta| meta.kind.clone())
}

/// Descends like the collector and checks the identity facts it relies on.
fn walk(root: Box<dyn AnchoredDirectory>) -> Result<BTreeSet<String>, String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![(String::new(), root)];
    while let Some((prefix, mut directory)) = stack.pop() {
        for entry in all(&mut *directory)? {
            let meta = entry.metadata.as_ref().map_err(|e| format!("{e:?}"))?;
            assert!(entry.enumerated_inode != 0 && entry.enumerated_inode == meta.inode);
            assert_eq!(entry.enumerated_kind, meta.kind);
            let relative = format!("{prefix}{}", String::from_utf8_lossy(&entry.name));
            if meta.kind == ObservedKind::Directory {
                stack.push((format!("{relative}/"), child(&*directory, &entry)?));
            }
            seen.insert(relative);
        }
    }
    Ok(seen)
}

/// Restores permissions so the temporary fixture can always be removed.
struct Restore(Vec<PathBuf>);
impl Drop for Restore {
    fn drop(&mut self) {
        for path in &self.0 {
            fs::set_permissions(path, Permissions::from_mode(0o755)).ok();
        }
    }
}

#[test]
fn links_are_observed_and_never_followed() -> TestResult {
    let (_guard, path) = fixture()?;
    let (_outside_guard, outside) = fixture()?;
    fs::write(outside.join("secret"), b"s")?;
    fs::create_dir_all(path.join("inner/deeper"))?;
    fs::write(path.join("inner/deeper/file"), b"f")?;
    symlink("..", path.join("up"))?;
    symlink(&outside, path.join("out"))?;
    symlink("inner/deeper/file", path.join("flink"))?;
    symlink("inner", path.join("dlink"))?;
    let mut root = open(&path)?;
    let entries = all(&mut *root.directory)?;
    for name in ["up", "out", "flink", "dlink"] {
        let entry = find(&entries, name)?;
        assert_eq!(entry.enumerated_kind, ObservedKind::Link);
        assert_eq!(kind(entry), Some(ObservedKind::Link));
        assert_eq!(
            entry.enumerated_inode,
            fs::symlink_metadata(path.join(name))?.ino()
        );
        refused(&*root.directory, entry)?;
    }
    let visited = walk(open(&path)?.directory)?;
    let expected: BTreeSet<String> = [
        "dlink",
        "flink",
        "inner",
        "inner/deeper",
        "inner/deeper/file",
        "out",
        "up",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(visited, expected);
    for through in ["dlink", "dlink/deeper", "out"] {
        let error = MacScopeRoots
            .open_root(path.join(through).as_os_str().as_bytes())
            .err()
            .ok_or("a link in the scope path was followed")?;
        assert_eq!(
            (error.kind, error.reason_key.as_str()),
            (PortErrorKind::Unsupported, "path.unresolvable")
        );
    }
    Ok(())
}

#[test]
fn permission_denial_stays_on_its_own_path() -> TestResult {
    let (_guard, path) = fixture()?;
    fs::write(path.join("probe"), b"")?;
    if fs::metadata(path.join("probe"))?.uid() == 0 {
        // The superuser bypasses permission bits, so the fixture cannot deny access.
        return Ok(());
    }
    for name in ["locked", "nosearch", "open"] {
        fs::create_dir(path.join(name))?;
        fs::write(path.join(name).join("inside"), b"x")?;
    }
    let _restore = Restore(vec![path.join("locked"), path.join("nosearch")]);
    fs::set_permissions(path.join("locked"), Permissions::from_mode(0o000))?;
    fs::set_permissions(path.join("nosearch"), Permissions::from_mode(0o444))?;
    let mut root = open(&path)?;
    let entries = all(&mut *root.directory)?;
    for name in ["locked", "nosearch", "open"] {
        assert_eq!(kind(find(&entries, name)?), Some(ObservedKind::Directory));
    }
    let locked = refused(&*root.directory, find(&entries, "locked")?)?;
    assert_eq!(
        (locked.kind, locked.reason_key.as_str()),
        (PortErrorKind::PermissionDenied, "os.query_failed")
    );
    let mut nosearch = child(&*root.directory, find(&entries, "nosearch")?)?;
    let listing = nosearch.next_batch().err().ok_or("listing was allowed")?;
    assert_eq!(listing.kind, PortErrorKind::PermissionDenied);
    let mut readable = child(&*root.directory, find(&entries, "open")?)?;
    assert_eq!(all(&mut *readable)?.len(), 1);
    let denied_root = MacScopeRoots
        .open_root(path.join("locked/inside").as_os_str().as_bytes())
        .err()
        .ok_or("denied path was opened")?;
    assert_eq!(denied_root.kind, PortErrorKind::PermissionDenied);
    Ok(())
}

#[test]
fn vanished_and_replaced_items_are_not_entered() -> TestResult {
    let (_guard, path) = fixture()?;
    for name in ["gone", "swap", "tolink", "other", "keep"] {
        fs::create_dir(path.join(name))?;
    }
    fs::write(path.join("replaced"), b"old")?;
    let mut root = open(&path)?;
    let entries = all(&mut *root.directory)?;
    let old_file = find(&entries, "replaced")?.enumerated_inode;
    fs::remove_dir(path.join("gone"))?;
    fs::rename(path.join("swap"), path.join("swapped"))?;
    fs::create_dir(path.join("swap"))?;
    fs::remove_dir(path.join("tolink"))?;
    symlink("other", path.join("tolink"))?;
    fs::rename(path.join("replaced"), path.join("saved"))?;
    fs::write(path.join("replaced"), b"new")?;
    let parent = &*root.directory;
    assert_eq!(
        refused(parent, find(&entries, "gone")?)?.kind,
        PortErrorKind::NotFound
    );
    assert_eq!(
        refused(parent, find(&entries, "swap")?)?.kind,
        PortErrorKind::IdentityChanged
    );
    let link = refused(parent, find(&entries, "tolink")?)?;
    assert_eq!(
        (link.kind, link.reason_key.as_str()),
        (PortErrorKind::Unsupported, "path.unresolvable")
    );
    child(parent, find(&entries, "keep")?)?;
    let fresh = all(&mut *open(&path)?.directory)?;
    assert_ne!(find(&fresh, "replaced")?.enumerated_inode, old_file);
    Ok(())
}

#[test]
fn deleting_entries_between_bulk_reads_ends_the_directory_cleanly() -> TestResult {
    let (_guard, path) = fixture()?;
    let long = "d".repeat(160);
    let names: Vec<_> = (0..1200).map(|n| format!("{long}{n:04}")).collect();
    for name in &names {
        fs::write(path.join(name), b"")?;
    }
    let mut directory = open(&path)?.directory;
    let first = directory.next_batch().map_err(|e| format!("{e:?}"))?.len();
    for name in &names {
        fs::remove_file(path.join(name))?;
    }
    let rest = all(&mut *directory)?.len();
    assert!(first > 0 && first <= 256);
    assert!(first + rest < names.len());
    Ok(())
}

#[test]
fn filtered_enumeration_decides_names_before_recording() -> TestResult {
    let (_guard, path) = fixture()?;
    for n in 0..300 {
        fs::write(path.join(format!("keep-{n}")), b"")?;
        fs::write(path.join(format!("skip-{n}")), b"")?;
    }
    let mut directory = open(&path)?.directory;
    let mut asked = 0usize;
    let mut kept = Vec::new();
    loop {
        let mut decide = |name: &[u8]| {
            asked += 1;
            if name.starts_with(b"skip-") {
                FilterDecision::Exclude
            } else {
                FilterDecision::Admit
            }
        };
        match directory
            .next_filtered(&mut decide)
            .map_err(|e| format!("{e:?}"))?
        {
            FilteredBatch::Entries(entries) => {
                assert!(!entries.is_empty() && entries.len() <= 256);
                kept.extend(entries.into_iter().map(|entry| entry.name));
            }
            FilteredBatch::Progress => {}
            FilteredBatch::End => break,
            FilteredBatch::Undecidable => return Err("unexpected undecidable".into()),
        }
    }
    assert_eq!((kept.len(), asked), (300, 600));
    assert!(kept.iter().all(|name| name.starts_with(b"keep-")));
    assert!(directory.next_batch().is_err(), "modes cannot be mixed");
    let mut excluded = open(&path)?.directory;
    loop {
        match excluded
            .next_filtered(&mut |_| FilterDecision::Exclude)
            .map_err(|e| format!("{e:?}"))?
        {
            FilteredBatch::Progress => {}
            FilteredBatch::End => break,
            _ => return Err("excluded entries were returned".into()),
        }
    }
    let mut undecided = open(&path)?.directory;
    for decision in [FilterDecision::Undecidable, FilterDecision::Admit] {
        let batch = undecided
            .next_filtered(&mut |_| decision)
            .map_err(|e| format!("{e:?}"))?;
        assert!(matches!(batch, FilteredBatch::Undecidable));
    }
    Ok(())
}
