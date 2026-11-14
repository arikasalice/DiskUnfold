#![cfg(target_os = "macos")]
//! macOS observation adapter checks on isolated temporary fixtures.
use du_core_contracts::{
    Timestamp,
    ports::PortErrorKind,
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, MetadataObservation, ObservedEntry, ObservedKind,
        OpenedScopeRoot, ScopeRootPort,
    },
};
use du_platform_os::macos_observation::MacScopeRoots;
use std::{
    ffi::OsStr,
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A temporary directory addressed by its resolved path, since /var is a link.
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

fn kind(entry: &ObservedEntry) -> Option<ObservedKind> {
    entry.metadata.as_ref().ok().map(|meta| meta.kind.clone())
}

#[test]
fn root_identity_comes_from_the_opened_handle() -> TestResult {
    let (_guard, path) = fixture()?;
    let root = open(&path)?;
    let expected = fs::metadata(&path)?;
    assert_eq!(root.canonical_path.display, path.to_string_lossy());
    assert!(!root.canonical_path.lossy);
    assert_eq!(root.file_id, expected.ino().to_le_bytes());
    assert!(root.volume_id.starts_with(b"uuid:") || root.volume_id.starts_with(b"dev:"));
    let millis = expected.mtime() * 1000 + expected.mtime_nsec().div_euclid(1_000_000);
    assert_eq!(
        root.directory.observed_modified_at(),
        Timestamp::from_unix_millis(millis)
    );
    if let Some(ids) = root.directory.ancestor_file_ids() {
        assert!(ids.iter().all(|id| !id.is_empty() && id != &root.file_id));
    }
    for rejected in [&b"relative"[..], b"/private/../private", b"/private/./tmp"] {
        assert!(MacScopeRoots.open_root(rejected).is_err());
    }
    let file = path.join("file");
    fs::write(&file, b"x")?;
    assert!(open(&file).is_err(), "a file is not a scope root");
    Ok(())
}

#[test]
fn normal_tree_reports_sizes_times_and_identities() -> TestResult {
    let (_guard, path) = fixture()?;
    fs::create_dir_all(path.join("a/b"))?;
    let content = path.join("a/b/c.txt");
    fs::write(&content, b"abc")?;
    fs::File::options()
        .write(true)
        .open(&content)?
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_600_000_000))?;
    fs::hard_link(&content, path.join("hard"))?;
    fs::write(path.join("empty"), b"")?;
    fs::create_dir(path.join("emptydir"))?;
    let mut root = open(&path)?;
    let entries = all(&mut *root.directory)?;
    let mut names: Vec<_> = entries.iter().map(|entry| entry.name.clone()).collect();
    names.sort();
    assert_eq!(names, [&b"a"[..], b"empty", b"emptydir", b"hard"]);
    let device = meta(entries.first().ok_or("no entries")?)?.device;
    for entry in &entries {
        let observed = meta(entry)?;
        let actual = fs::symlink_metadata(path.join(OsStr::from_bytes(&entry.name)))?;
        assert_eq!(
            (entry.enumerated_inode, observed.inode),
            (actual.ino(), actual.ino())
        );
        assert_eq!(entry.enumerated_kind, observed.kind);
        assert_eq!(observed.device, device);
    }
    let empty = meta(find(&entries, b"empty")?)?;
    assert_eq!((empty.kind, empty.size), (ObservedKind::File, Ok(0)));
    let mut emptydir = child(&*root.directory, find(&entries, b"emptydir")?)?;
    assert!(
        emptydir
            .next_batch()
            .map_err(|e| format!("{e:?}"))?
            .is_empty()
    );
    assert!(
        emptydir
            .next_batch()
            .map_err(|e| format!("{e:?}"))?
            .is_empty()
    );
    let mut a = child(&*root.directory, find(&entries, b"a")?)?;
    let a_entries = all(&mut *a)?;
    let mut b = child(&*a, find(&a_entries, b"b")?)?;
    let file = meta(find(&all(&mut *b)?, b"c.txt")?)?;
    assert_eq!(file.size, Ok(3));
    assert!(file.allocated.is_some_and(|bytes| bytes > 0));
    assert_eq!(file.link_count, Some(2));
    assert_eq!(
        file.modified_at.as_ref().map(Timestamp::as_str),
        Some("2020-09-13T12:26:40.000Z")
    );
    let identity = file.space_identity.ok_or("missing space identity")?;
    assert_eq!(identity.volume, root.volume_id);
    assert_eq!(identity.file, fs::metadata(&content)?.ino().to_le_bytes());
    let hard = meta(find(&entries, b"hard")?)?;
    assert_eq!(hard.space_identity.map(|id| id.file), Some(identity.file));
    Ok(())
}

#[test]
fn large_directories_return_bounded_batches_across_bulk_calls() -> TestResult {
    let (_guard, path) = fixture()?;
    for n in 0..300 {
        fs::write(path.join(format!("f{n}")), b"abc")?;
    }
    let mut root = open(&path)?;
    let mut sizes = Vec::new();
    for _ in 0..3 {
        sizes.push(
            root.directory
                .next_batch()
                .map_err(|e| format!("{e:?}"))?
                .len(),
        );
    }
    assert_eq!(sizes, [256, 44, 0]);
    let wide = path.join("wide");
    fs::create_dir(&wide)?;
    let long = "n".repeat(160);
    for n in 0..2000 {
        fs::write(wide.join(format!("{long}{n:04}")), b"")?;
    }
    let mut directory = open(&wide)?.directory;
    let mut seen = std::collections::BTreeSet::new();
    let mut batches = 0;
    loop {
        let batch = directory.next_batch().map_err(|e| format!("{e:?}"))?;
        if batch.is_empty() {
            break;
        }
        assert!(batch.len() <= 256);
        batches += 1;
        seen.extend(batch.into_iter().map(|entry| entry.name));
    }
    assert_eq!(seen.len(), 2000);
    assert!(batches >= 8);
    Ok(())
}

#[test]
fn names_keep_the_bytes_the_volume_stores() -> TestResult {
    let (_guard, path) = fixture()?;
    let nfc = "x-\u{e9}".as_bytes().to_vec();
    let nfd = "x-e\u{301}".as_bytes().to_vec();
    for (folder, name) in [("nfc", &nfc), ("nfd", &nfd)] {
        fs::create_dir(path.join(folder))?;
        fs::write(path.join(folder).join(OsStr::from_bytes(name)), b"")?;
        let mut directory = open(&path.join(folder))?.directory;
        let names: Vec<_> = all(&mut *directory)?.into_iter().map(|e| e.name).collect();
        assert_eq!(names, std::slice::from_ref(name));
    }
    let both = path.join("both");
    fs::create_dir(&both)?;
    fs::write(both.join(OsStr::from_bytes(&nfc)), b"")?;
    let second = fs::File::create_new(both.join(OsStr::from_bytes(&nfd)));
    let mut directory = open(&both)?.directory;
    let mut names: Vec<_> = all(&mut *directory)?.into_iter().map(|e| e.name).collect();
    names.sort();
    match second {
        // A normalization-insensitive volume keeps only the first stored form.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            assert_eq!(names, std::slice::from_ref(&nfc));
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {
            let mut expected = vec![nfc.clone(), nfd.clone()];
            expected.sort();
            assert_eq!(names, expected);
        }
    }
    let folder = path.join(OsStr::from_bytes(&nfd));
    fs::create_dir(&folder)?;
    fs::write(folder.join("inside"), b"")?;
    let mut root = open(&path)?;
    let entries = all(&mut *root.directory)?;
    let mut inner = child(&*root.directory, find(&entries, &nfd)?)?;
    assert_eq!(all(&mut *inner)?.len(), 1);
    Ok(())
}

#[test]
fn volume_space_is_bound_to_the_root_volume() -> TestResult {
    let (_guard, path) = fixture()?;
    let root = open(&path)?;
    let reader = root
        .directory
        .volume_space_reader()
        .map_err(|e| format!("{e:?}"))?;
    let space = reader.read(&root.volume_id).map_err(|e| format!("{e:?}"))?;
    assert!(space.total_bytes > 0);
    assert!(space.free_bytes <= space.total_bytes);
    assert!(space.used_bytes <= space.total_bytes);
    let wrong = reader
        .read(b"uuid:other")
        .err()
        .ok_or("another volume was accepted")?;
    assert_eq!(wrong.kind, PortErrorKind::IdentityChanged);
    assert_eq!(wrong.reason_key, "space.volume_report_unavailable");
    Ok(())
}

/// Uses the read-only system volume layout of macOS 10.15 and later.
#[test]
fn firmlinks_and_mount_points_are_links_that_are_never_entered() -> TestResult {
    let mut system = open(Path::new("/"))?;
    assert_eq!(system.directory.ancestor_file_ids(), Some(&[][..]));
    let entries = all(&mut *system.directory)?;
    let users = find(&entries, b"Users")?;
    assert_eq!(users.enumerated_kind, ObservedKind::Link);
    assert_eq!(kind(users), Some(ObservedKind::Link));
    assert!(system.directory.open_child(users).is_err());
    assert_eq!(
        kind(find(&entries, b"System")?),
        Some(ObservedKind::Directory)
    );
    let mut volumes = open(Path::new("/System/Volumes"))?;
    let entries = all(&mut *volumes.directory)?;
    let data = find(&entries, b"Data")?;
    assert_eq!(kind(data), Some(ObservedKind::Link));
    assert!(volumes.directory.open_child(data).is_err());
    let data_root = open(Path::new("/System/Volumes/Data"))?;
    assert_eq!(data_root.directory.ancestor_file_ids(), Some(&[][..]));
    assert_ne!(data_root.volume_id, system.volume_id);
    let library = open(Path::new("/System/Library"))?;
    assert_eq!(
        library.directory.ancestor_file_ids().map(<[Vec<u8>]>::len),
        Some(2)
    );
    Ok(())
}
