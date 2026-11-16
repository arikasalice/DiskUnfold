//! Adapter behavior on real temporary directories.
use super::{LinuxScopeRoots, enumeration::BATCH_SIZE, metadata::PROBES};
use du_core_contracts::{
    PathEncoding,
    ports::{PortError, PortErrorKind},
    scan_observation::{
        AnchoredDirectory, FilterDecision, FilteredBatch, ObservedEntry, ObservedKind,
        OpenedScopeRoot, ScopeRootPort,
    },
};
use std::{
    ffi::OsStr,
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, symlink},
    },
    path::Path,
};

mod boundary;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fail(error: PortError) -> String {
    format!("{error:?}")
}

fn link_creation_is_unavailable(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
    )
}

fn open(path: &Path) -> Result<OpenedScopeRoot, String> {
    LinuxScopeRoots
        .open_root(path.as_os_str().as_bytes())
        .map_err(fail)
}

fn drain(directory: &mut dyn AnchoredDirectory) -> Result<Vec<ObservedEntry>, String> {
    let mut all = Vec::new();
    loop {
        let batch = directory.next_batch().map_err(fail)?;
        if batch.is_empty() {
            return Ok(all);
        }
        assert!(batch.len() <= BATCH_SIZE);
        all.extend(batch);
    }
}

fn entry<'a>(entries: &'a [ObservedEntry], name: &[u8]) -> Result<&'a ObservedEntry, String> {
    entries
        .iter()
        .find(|item| item.name == name)
        .ok_or_else(|| format!("missing {}", String::from_utf8_lossy(name)))
}

fn raw(root: &OpenedScopeRoot) -> Result<Vec<u8>, String> {
    root.canonical_path
        .raw
        .bytes()
        .map_err(|e| format!("{e:?}"))
}

fn tree() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let base = temp.path();
    fs::write(base.join("a.txt"), b"hello")?;
    fs::hard_link(base.join("a.txt"), base.join("a-link"))?;
    fs::write(base.join("zero"), b"")?;
    fs::create_dir(base.join("sub"))?;
    fs::write(base.join("sub").join("b.bin"), vec![7u8; 5000])?;
    fs::create_dir(base.join("empty"))?;
    Ok(temp)
}

#[test]
fn reports_root_identity_and_file_facts() -> TestResult {
    let temp = tree()?;
    let base = temp.path();
    let mut root = open(base)?;
    let info = fs::metadata(base)?;
    assert_eq!(root.canonical_path.encoding, PathEncoding::Unix);
    assert_eq!(raw(&root)?, base.as_os_str().as_bytes());
    assert_eq!(root.file_id, info.ino().to_le_bytes());
    assert!(root.volume_id.starts_with(b"linux:"));
    assert!(root.directory.observed_modified_at().is_some());
    let entries = drain(&mut *root.directory)?;
    assert_eq!(entries.len(), 5);
    let file = entry(&entries, b"a.txt")?;
    let meta = file.metadata.as_ref().map_err(|e| fail(e.clone()))?;
    assert_eq!(
        (&meta.kind, &file.enumerated_kind),
        (&ObservedKind::File, &ObservedKind::File)
    );
    assert_eq!(meta.inode, fs::metadata(base.join("a.txt"))?.ino());
    assert_eq!(file.enumerated_inode, meta.inode);
    assert_eq!(meta.device, info.dev());
    assert_eq!(meta.size, Ok(5));
    assert_eq!(meta.link_count, Some(2));
    assert!(meta.allocated.is_some_and(|bytes| bytes % 512 == 0));
    assert!(meta.modified_at.is_some());
    let identity = meta
        .space_identity
        .clone()
        .ok_or("missing space identity")?;
    assert_eq!(identity.volume, root.volume_id);
    assert_eq!(identity.file, meta.inode.to_le_bytes());
    let alias = entry(&entries, b"a-link")?
        .metadata
        .as_ref()
        .map_err(|e| fail(e.clone()))?;
    assert_eq!(alias.space_identity, meta.space_identity);
    let zero = entry(&entries, b"zero")?
        .metadata
        .as_ref()
        .map_err(|e| fail(e.clone()))?;
    assert_eq!((zero.size.clone(), zero.link_count), (Ok(0), Some(1)));
    assert!(root.directory.open_child(file).is_err());
    Ok(())
}

#[test]
fn opens_child_and_empty_directories_through_their_entries() -> TestResult {
    let temp = tree()?;
    let mut root = open(temp.path())?;
    let entries = drain(&mut *root.directory)?;
    let sub = entry(&entries, b"sub")?;
    let sub_meta = sub.metadata.as_ref().map_err(|e| fail(e.clone()))?;
    assert_eq!(sub_meta.kind, ObservedKind::Directory);
    assert_eq!((sub_meta.allocated, sub_meta.link_count), (None, None));
    assert!(sub_meta.space_identity.is_none());
    let mut child = root.directory.open_child(sub).map_err(fail)?;
    let nested = drain(&mut *child)?;
    let large = entry(&nested, b"b.bin")?
        .metadata
        .as_ref()
        .map_err(|e| fail(e.clone()))?;
    assert_eq!(large.size, Ok(5000));
    let mut empty = root
        .directory
        .open_child(entry(&entries, b"empty")?)
        .map_err(fail)?;
    assert!(drain(&mut *empty)?.is_empty());
    Ok(())
}

#[test]
fn reports_handle_identity_of_roots_and_children() -> TestResult {
    let temp = tempfile::tempdir()?;
    let nested = temp.path().join("one").join("two");
    fs::create_dir_all(nested.join("child"))?;
    let mut root = open(&nested)?;
    let info = fs::metadata(&nested)?;
    let identity = root
        .directory
        .observed_identity()
        .ok_or("missing root identity")?;
    assert_eq!((identity.device, identity.inode), (info.dev(), info.ino()));
    let retained = root
        .directory
        .ancestor_file_ids()
        .map_or_else(Vec::new, <[_]>::to_vec)
        .into_iter()
        .map(|id| <[u8; 8]>::try_from(id.as_slice()).map(u64::from_le_bytes))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(identity.ancestors, retained);
    if root.directory.ancestor_file_ids().is_some() {
        let parent = fs::metadata(temp.path().join("one"))?.ino();
        assert_eq!(identity.ancestors.last(), Some(&parent));
    }
    let entries = drain(&mut *root.directory)?;
    let child = entry(&entries, b"child")?;
    let meta = child.metadata.as_ref().map_err(|e| fail(e.clone()))?;
    // Children are compared with the root identity in the same device space.
    assert_eq!(meta.device, identity.device);
    let opened = root.directory.open_child(child).map_err(fail)?;
    let child_identity = opened.observed_identity().ok_or("missing child identity")?;
    assert_eq!(
        (child_identity.device, child_identity.inode),
        (identity.device, meta.inode)
    );
    assert!(child_identity.ancestors.is_empty());
    Ok(())
}

#[test]
fn retains_same_mount_ancestors_and_reads_volume_space() -> TestResult {
    let temp = tempfile::tempdir()?;
    let nested = temp.path().join("one").join("two");
    fs::create_dir_all(&nested)?;
    let root = open(&nested)?;
    match root.directory.ancestor_file_ids() {
        Some(ids) => {
            assert!(ids.iter().all(|id| !id.is_empty() && *id != root.file_id));
            let parent = fs::metadata(temp.path().join("one"))?.ino().to_le_bytes();
            assert_eq!(ids.last().map(Vec::as_slice), Some(parent.as_slice()));
            let system = open(Path::new("/"))?;
            assert_eq!(
                system.directory.ancestor_file_ids().map(<[_]>::len),
                Some(0)
            );
            assert_eq!(raw(&system)?, b"/");
        }
        None => eprintln!("F02_LINUX mount IDs unavailable; ancestor evidence withheld"),
    }
    let reader = root.directory.volume_space_reader().map_err(fail)?;
    let space = reader.read(&root.volume_id).map_err(fail)?;
    assert!(space.free_bytes <= space.total_bytes);
    assert_eq!(space.used_bytes + space.free_bytes, space.total_bytes);
    let other = reader
        .read(b"linux:0:0:0")
        .err()
        .ok_or("accepted another volume")?;
    assert_eq!(
        (other.kind, other.reason_key.as_str()),
        (
            PortErrorKind::IdentityChanged,
            "space.volume_report_unavailable"
        )
    );
    Ok(())
}

#[test]
fn normalizes_paths_and_refuses_links_parents_and_files() -> TestResult {
    let temp = tempfile::tempdir()?;
    let base = temp.path();
    fs::create_dir(base.join("real"))?;
    let alias_created = match symlink(base.join("real"), base.join("alias")) {
        Ok(()) => true,
        Err(error) if link_creation_is_unavailable(&error) => {
            eprintln!("skipped Linux symlink-path assertions after creation failed: {error}");
            false
        }
        Err(error) => return Err(error.into()),
    };
    fs::write(base.join("file"), b"x")?;
    let prefix = base.as_os_str().as_bytes();
    let with = |suffix: &[u8]| [prefix, suffix].concat();
    let messy = LinuxScopeRoots
        .open_root(&with(b"//./real/"))
        .map_err(fail)?;
    assert_eq!(raw(&messy)?, with(b"/real"));
    let refused = |path: &[u8]| {
        LinuxScopeRoots
            .open_root(path)
            .err()
            .map(|error| (error.kind, error.reason_key))
    };
    let unresolvable = Some((PortErrorKind::Unsupported, "path.unresolvable".to_owned()));
    let mut refused_paths: Vec<&[u8]> = vec![b"/real/..", b"/file", b"/real\0x"];
    if alias_created {
        refused_paths.extend([&b"/alias"[..], b"/alias/."]);
    }
    for suffix in refused_paths {
        assert_eq!(refused(&with(suffix)), unresolvable, "{suffix:?}");
    }
    assert_eq!(refused(b"relative/path"), unresolvable);
    assert_eq!(
        refused(&with(b"/missing")).map(|(kind, _)| kind),
        Some(PortErrorKind::NotFound)
    );
    Ok(())
}

/// A Windows path whose UTF-16LE units include ASCII has NUL bytes or does
/// not start with `/`, so it is refused before any directory is opened.
#[test]
fn refuses_windows_encoded_paths() {
    let wide = |text: &str| -> Vec<u8> { text.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    for text in ["C:\\data", "\\\\?\\C:\\data", "/tmp", "/"] {
        let refused = LinuxScopeRoots
            .open_root(&wide(text))
            .err()
            .map(|error| (error.kind, error.reason_key));
        assert_eq!(
            refused,
            Some((PortErrorKind::Unsupported, "path.unresolvable".to_owned())),
            "{text}"
        );
    }
}

#[test]
fn streams_raw_names_in_bounded_batches() -> TestResult {
    let temp = tempfile::tempdir()?;
    let base = temp.path().join(OsStr::from_bytes(b"root-\xfe"));
    fs::create_dir(&base)?;
    for index in 0..300 {
        fs::write(base.join(format!("f{index}")), b"abc")?;
    }
    fs::write(base.join(OsStr::from_bytes(b"raw-\xff")), b"x")?;
    let mut root = open(&base)?;
    assert!(root.canonical_path.lossy);
    assert_eq!(raw(&root)?, base.as_os_str().as_bytes());
    let first = root.directory.next_batch().map_err(fail)?;
    assert_eq!(first.len(), BATCH_SIZE);
    let rest = drain(&mut *root.directory)?;
    assert_eq!(first.len() + rest.len(), 301);
    assert!(
        first
            .iter()
            .chain(&rest)
            .any(|item| item.name == b"raw-\xff" && item.metadata.is_ok())
    );
    Ok(())
}

#[test]
fn observes_links_without_following_or_revisiting() -> TestResult {
    let temp = tempfile::tempdir()?;
    let base = temp.path().join("scope");
    fs::create_dir_all(base.join("sub"))?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;
    fs::write(target.join("target-only"), vec![7u8; 4096])?;
    if let Err(error) = symlink("..", base.join("up")) {
        if link_creation_is_unavailable(&error) {
            eprintln!("skipped Linux symbolic-link case after symlink creation failed: {error}");
            return Ok(());
        }
        return Err(error.into());
    }
    symlink("../..", base.join("sub").join("loop"))?;
    symlink("/", base.join("outside"))?;
    symlink("missing", base.join("dangling"))?;
    symlink("../target", base.join("target-link"))?;
    let mut stack = vec![(open(&base)?.directory, base.clone())];
    let mut visited = 0;
    let mut links = Vec::new();
    let mut observed_names = Vec::new();
    while let Some((mut directory, directory_path)) = stack.pop() {
        visited += 1;
        for item in drain(&mut *directory)? {
            observed_names.push(item.name.clone());
            match item.metadata.as_ref().map(|meta| meta.kind.clone()) {
                Ok(ObservedKind::Directory) => {
                    let child_path = directory_path.join(OsStr::from_bytes(&item.name));
                    let child = directory.open_child(&item).map_err(fail)?;
                    stack.push((child, child_path));
                }
                Ok(ObservedKind::Link) => {
                    assert_eq!(item.enumerated_kind, ObservedKind::Link);
                    let metadata = item
                        .metadata
                        .as_ref()
                        .map_err(|error| fail(error.clone()))?;
                    if let Ok(size) = &metadata.size {
                        let own_size = fs::symlink_metadata(
                            directory_path.join(OsStr::from_bytes(&item.name)),
                        )?
                        .len();
                        assert_eq!(*size, own_size, "reported size belongs to the link itself");
                    }
                    assert!(directory.open_child(&item).is_err());
                    links.push(item.name);
                }
                other => return Err(format!("unexpected {other:?}").into()),
            }
        }
    }
    assert_eq!(visited, 2);
    assert!(!observed_names.iter().any(|name| name == b"target-only"));
    links.sort();
    let expected: [&[u8]; 5] = [b"dangling", b"loop", b"outside", b"target-link", b"up"];
    assert_eq!(links, expected);
    Ok(())
}

#[test]
fn filters_names_before_any_metadata_query() -> TestResult {
    let temp = tempfile::tempdir()?;
    for name in ["keep-1", "keep-2", "skip-1", "skip-2", "skip-3"] {
        fs::write(temp.path().join(name), b"x")?;
    }
    let mut root = open(temp.path())?;
    PROBES.with(|count| count.set(0));
    let mut include = |name: &[u8]| {
        if name.starts_with(b"skip") {
            FilterDecision::Exclude
        } else {
            FilterDecision::Admit
        }
    };
    let mut seen = Vec::new();
    loop {
        match root.directory.next_filtered(&mut include).map_err(fail)? {
            FilteredBatch::Entries(items) => seen.extend(items.into_iter().map(|item| item.name)),
            FilteredBatch::Progress => {}
            FilteredBatch::End => break,
            FilteredBatch::Undecidable => return Err("undecidable".into()),
        }
    }
    seen.sort();
    let expected: [&[u8]; 2] = [b"keep-1", b"keep-2"];
    assert_eq!(seen, expected);
    assert_eq!(PROBES.with(std::cell::Cell::get), 2);
    assert!(root.directory.next_batch().is_err());
    let mut undecided = open(temp.path())?;
    for _ in 0..2 {
        let batch = undecided
            .directory
            .next_filtered(&mut |_| FilterDecision::Undecidable)
            .map_err(fail)?;
        assert!(matches!(batch, FilteredBatch::Undecidable));
    }
    Ok(())
}
