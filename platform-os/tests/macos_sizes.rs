#![cfg(target_os = "macos")]
//! Logical and allocated sizes of macOS observations compared with `lstat`.
//!
//! `size` must be `st_size` of the entry itself, as on Linux and Windows,
//! and `allocated` must be `st_blocks` in bytes, which `du` counts, for files.
use du_core_contracts::scan_observation::{
    MetadataObservation, ObservedEntry, ObservedKind, ScopeRootPort,
};
use du_platform_os::macos_observation::MacScopeRoots;
use std::{
    fs,
    io::{Seek, SeekFrom, Write},
    os::unix::{ffi::OsStrExt, fs::MetadataExt, fs::symlink},
    path::Path,
    process::Command,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// `UF_COMPRESSED` from <sys/stat.h>.
const UF_COMPRESSED: u32 = 0x20;

fn entries(path: &Path) -> Result<Vec<ObservedEntry>, String> {
    let mut root = MacScopeRoots
        .open_root(path.as_os_str().as_bytes())
        .map_err(|e| format!("{e:?}"))?;
    let mut entries = Vec::new();
    loop {
        let batch = root.directory.next_batch().map_err(|e| format!("{e:?}"))?;
        if batch.is_empty() {
            return Ok(entries);
        }
        entries.extend(batch);
    }
}

fn meta<'a>(entries: &'a [ObservedEntry], name: &str) -> Result<&'a MetadataObservation, String> {
    entries
        .iter()
        .find(|entry| entry.name == name.as_bytes())
        .ok_or(format!("missing {name}"))?
        .metadata
        .as_ref()
        .map_err(|e| format!("{name}: {e:?}"))
}

#[test]
fn sizes_match_lstat_for_forks_holes_directories_and_links() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = fs::canonicalize(directory.path())?;
    fs::write(path.join("forked"), b"data")?;
    fs::write(path.join("forked/..namedfork/rsrc"), vec![7u8; 20_000])?;
    let mut sparse = fs::File::create(path.join("sparse"))?;
    sparse.seek(SeekFrom::Start(64 << 20))?;
    sparse.write_all(b"tail")?;
    sparse.sync_all()?;
    fs::create_dir(path.join("folder"))?;
    for n in 0..5 {
        fs::write(path.join("folder").join(format!("f{n}")), b"")?;
    }
    symlink("folder/f0", path.join("link"))?;
    let entries = entries(&path)?;
    for name in ["forked", "sparse", "folder", "link"] {
        let observed = meta(&entries, name)?;
        let actual = fs::symlink_metadata(path.join(name))?;
        assert_eq!(observed.size, Ok(actual.size()), "{name}");
        let blocks = actual.blocks() * 512;
        let expected = (observed.kind == ObservedKind::File).then_some(blocks);
        assert_eq!(observed.allocated, expected, "{name}");
        eprintln!(
            "F02_MACOS_SIZE {name} kind={:?} size={:?} allocated={:?} st_size={} st_blocks_bytes={blocks}",
            observed.kind,
            observed.size,
            observed.allocated,
            actual.size()
        );
    }
    // The resource fork counts towards the allocation but not the length.
    let forked = meta(&entries, "forked")?;
    assert_eq!(forked.size, Ok(4));
    assert!(forked.allocated.is_some_and(|bytes| bytes >= 20_000));
    // A hole is part of the length but not of the allocation.
    let sparse = meta(&entries, "sparse")?;
    assert_eq!(sparse.size, Ok((64 << 20) + 4));
    assert!(sparse.allocated.is_some_and(|bytes| bytes < 1 << 20));
    assert_eq!(meta(&entries, "link")?.size, Ok(9));
    Ok(())
}

/// A file with transparent APFS compression: the length stays the logical
/// length and the allocation is the compressed one, as `du` reports.
#[test]
fn compressed_files_report_logical_length_and_compressed_allocation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = fs::canonicalize(directory.path())?;
    let plain = path.join("plain");
    fs::write(
        &plain,
        "DiskUnfold compression fixture line\n".repeat(30_000),
    )?;
    // Without --noclone, ditto clones on the same volume and compresses nothing.
    let copied = Command::new("ditto")
        .args(["--noclone", "--hfsCompression"])
        .arg(&plain)
        .arg(path.join("compressed"))
        .status();
    if !copied.as_ref().is_ok_and(|status| status.success()) {
        eprintln!("skipped: ditto could not compress a copy: {copied:?}");
        return Ok(());
    }
    let actual = fs::symlink_metadata(path.join("compressed"))?;
    if std::os::macos::fs::MetadataExt::st_flags(&actual) & UF_COMPRESSED == 0 {
        eprintln!("skipped: the volume kept the copy uncompressed");
        return Ok(());
    }
    let entries = entries(&path)?;
    let observed = meta(&entries, "compressed")?;
    let blocks = actual.blocks() * 512;
    eprintln!(
        "F02_MACOS_SIZE compressed size={:?} allocated={:?} st_size={} st_blocks_bytes={blocks}",
        observed.size,
        observed.allocated,
        actual.size()
    );
    assert_eq!(observed.size, Ok(actual.size()));
    assert_eq!(observed.size, Ok(1_080_000));
    assert_eq!(observed.allocated, Some(blocks));
    assert!(blocks < actual.size());
    Ok(())
}
