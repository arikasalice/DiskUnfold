#![cfg(target_os = "linux")]
//! #202, F03 FR11.HD1: the real Linux adapter confirms that a scope root's volume cannot
//! share blocks only on a read-write ext2, ext3 or ext4 mount.
//!
//! The expected answer comes from `/proc/self/mountinfo`, a source the adapter does not
//! read: the file system type and the read-only options of the mount that holds the path.
use du_core_contracts::scan_observation::ScopeRootPort;
use du_platform_os::linux_observation::LinuxScopeRoots;
use std::{fs, os::unix::ffi::OsStrExt, path::Path};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Undoes the octal escapes (`\040` for a space) of a mountinfo path field.
fn unescape(field: &str) -> Vec<u8> {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let octal = bytes.get(index + 1..index + 4).and_then(|digits| {
            let text = std::str::from_utf8(digits).ok()?;
            u8::from_str_radix(text, 8).ok()
        });
        match (bytes[index], octal) {
            (b'\\', Some(value)) => {
                out.push(value);
                index += 4;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    out
}

/// Whether the mount that holds `path` is a read-write ext2, ext3 or ext4 mount. The
/// last of the longest matching mount points wins, as a later mount covers an earlier one.
fn expected(path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let path = fs::canonicalize(path)?;
    let path = path.as_os_str().as_bytes();
    let table = fs::read_to_string("/proc/self/mountinfo")?;
    let mut best: Option<(usize, bool)> = None;
    for line in table.lines() {
        let (mount, filesystem) = line.split_once(" - ").ok_or("no separator")?;
        let mount: Vec<&str> = mount.split(' ').collect();
        let filesystem: Vec<&str> = filesystem.split(' ').collect();
        let point = unescape(mount.get(4).ok_or("no mount point")?);
        let covers = path.starts_with(&point)
            && (point == b"/" || path.len() == point.len() || path[point.len()] == b'/');
        if !covers || best.is_some_and(|(length, _)| point.len() < length) {
            continue;
        }
        let read_write = |options: Option<&&str>| {
            options.is_some_and(|options| options.split(',').any(|option| option == "rw"))
        };
        let ext = matches!(filesystem.first(), Some(&("ext2" | "ext3" | "ext4")));
        best = Some((
            point.len(),
            ext && read_write(mount.get(5)) && read_write(filesystem.get(2)),
        ));
    }
    Ok(best.ok_or("no mount holds the path")?.1)
}

#[test]
fn only_a_read_write_ext_mount_confirms_that_nothing_is_shared() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let workspace_temp = tempfile::tempdir_in(".");
    let mut candidates = vec![temporary.path().to_path_buf()];
    if let Ok(ref dir) = workspace_temp {
        candidates.push(dir.path().to_path_buf());
    }
    candidates.extend(
        ["/dev/shm", "/run", "/var/tmp"]
            .into_iter()
            .map(Path::new)
            .filter(|path| path.is_dir())
            .map(Path::to_path_buf),
    );
    let mut confirmed = 0;
    let mut has_ext_candidate = false;
    for path in &candidates {
        let root = LinuxScopeRoots
            .open_root(path.as_os_str().as_bytes())
            .map_err(|error| format!("{}: {error:?}", path.display()))?;
        let answer = root.directory.creates_no_shared_extents();
        let is_expected_ext = expected(path)?;
        has_ext_candidate |= is_expected_ext;
        eprintln!(
            "F03_LINUX_SHARING path={} unshared={answer} expected_ext={is_expected_ext}",
            path.display()
        );
        assert_eq!(answer, is_expected_ext, "{}", path.display());
        confirmed += usize::from(answer);
    }
    // GitHub-hosted Linux runners keep the temporary folder on the ext4 root volume, whereas
    // self-hosted Docker container runners use overlayfs for `/` and tmpfs for `/tmp`. Assert
    // a positive confirmation whenever any candidate resides on a read-write ext mount or on
    // GitHub-hosted runners.
    if has_ext_candidate
        || (std::env::var_os("GITHUB_ACTIONS").is_some()
            && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted"))
    {
        assert!(confirmed > 0, "no candidate was on a read-write ext mount");
    }
    Ok(())
}
