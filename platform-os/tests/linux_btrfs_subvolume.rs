#![cfg(target_os = "linux")]
//! btrfs subvolumes observed through the real Linux adapter.
//!
//! The ignored test runs only on a GitHub Ubuntu runner. It formats a sparse
//! image as btrfs, mounts it through a loop device with `sudo` and creates
//! subvolumes, which the runner's ext4 root cannot hold.
use du_core_contracts::scan_observation::{
    AnchoredDirectory, ObservedEntry, ObservedKind, OpenedScopeRoot, ScopeRootPort,
};
use du_platform_os::linux_observation::LinuxScopeRoots;
use std::{
    env,
    ffi::OsStr,
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    process::Command,
};

type Failure = Box<dyn std::error::Error>;
type TestResult = Result<(), Failure>;

/// Size of the sparse image; mkfs.btrfs needs more than 109 MiB.
const IMAGE_BYTES: u64 = 256 << 20;

fn is_github_linux_runner() -> bool {
    env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true")
        && env::var("RUNNER_OS").is_ok_and(|value| value == "Linux")
        && env::var_os("RUNNER_TEMP").is_some()
}

/// Runs `program` and fails with its standard error when it does not succeed.
fn command(program: &str, args: &[&OsStr]) -> TestResult {
    let output = Command::new(program).args(args).output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{program} {args:?} failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )
    .into())
}

/// Runs one command as root without prompting for a password.
fn sudo(args: &[&OsStr]) -> TestResult {
    let mut full = vec![OsStr::new("-n")];
    full.extend_from_slice(args);
    command("sudo", &full)
}

fn open(path: &Path) -> Result<OpenedScopeRoot, String> {
    LinuxScopeRoots
        .open_root(path.as_os_str().as_bytes())
        .map_err(|error| format!("{error:?}"))
}

fn drain(directory: &mut dyn AnchoredDirectory) -> Result<Vec<ObservedEntry>, String> {
    let mut all = Vec::new();
    loop {
        let batch = directory
            .next_batch()
            .map_err(|error| format!("{error:?}"))?;
        if batch.is_empty() {
            return Ok(all);
        }
        all.extend(batch);
    }
}

fn find<'a>(entries: &'a [ObservedEntry], name: &str) -> Result<&'a ObservedEntry, String> {
    entries
        .iter()
        .find(|entry| entry.name == name.as_bytes())
        .ok_or_else(|| format!("{name} was not enumerated"))
}

/// A btrfs volume with two subvolumes, one nested subvolume and a plain folder.
struct BtrfsFixture {
    image: PathBuf,
    mount: PathBuf,
    mounted: bool,
}

impl BtrfsFixture {
    fn new() -> Result<Self, Failure> {
        let temp = PathBuf::from(env::var_os("RUNNER_TEMP").ok_or("RUNNER_TEMP is not set")?);
        let id = std::process::id();
        let mut fixture = Self {
            image: temp.join(format!("du-btrfs-{id}.img")),
            mount: temp.join(format!("du-btrfs-{id}")),
            mounted: false,
        };
        // A leftover file of the same name fails the test instead of being reformatted.
        fs::File::create_new(&fixture.image)?.set_len(IMAGE_BYTES)?;
        fs::create_dir(&fixture.mount)?;
        command(
            "mkfs.btrfs",
            &["-q".as_ref(), "-f".as_ref(), fixture.image.as_os_str()],
        )?;
        sudo(&[
            "mount".as_ref(),
            "-o".as_ref(),
            "loop".as_ref(),
            fixture.image.as_os_str(),
            fixture.mount.as_os_str(),
        ])?;
        fixture.mounted = true;
        for name in ["first", "second", "second/nested"] {
            let path = fixture.mount.join(name);
            sudo(&[
                "btrfs".as_ref(),
                "subvolume".as_ref(),
                "create".as_ref(),
                path.as_os_str(),
            ])?;
        }
        let owner = fs::metadata(&fixture.image)?;
        let owner = format!("{}:{}", owner.uid(), owner.gid());
        sudo(&[
            "chown".as_ref(),
            "-R".as_ref(),
            owner.as_ref(),
            fixture.mount.as_os_str(),
        ])?;
        fs::create_dir(fixture.mount.join("plain"))?;
        fs::write(fixture.mount.join("plain").join("a.bin"), [1u8; 10])?;
        fs::write(fixture.mount.join("first").join("b.bin"), [2u8; 20])?;
        Ok(fixture)
    }

    fn cleanup(&mut self) -> TestResult {
        if self.mounted {
            // A busy mount is detached lazily so a reused runner keeps no mount.
            sudo(&["umount".as_ref(), self.mount.as_os_str()]).or_else(|error| {
                eprintln!("btrfs fixture umount failed, detaching lazily: {error}");
                sudo(&["umount".as_ref(), "-l".as_ref(), self.mount.as_os_str()])
            })?;
            self.mounted = false;
        }
        if self.mount.exists() {
            fs::remove_dir(&self.mount)?;
        }
        if self.image.exists() {
            fs::remove_file(&self.image)?;
        }
        Ok(())
    }
}

impl Drop for BtrfsFixture {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!("btrfs fixture cleanup failed: {error}");
        }
    }
}

/// Prints what enumeration and `statx` reported for each named subvolume.
fn report(entries: &[ObservedEntry], parent: &Path, names: &[&str]) -> TestResult {
    let parent_device = fs::metadata(parent)?.dev();
    for name in names {
        let entry = find(entries, name)?;
        let stat = fs::symlink_metadata(parent.join(name))?;
        eprintln!(
            "F02_LINUX_BTRFS name={name} parent_device={parent_device} stat_device={} \
             stat_inode={} enumerated_kind={:?} enumerated_inode={} observed={:?}",
            stat.dev(),
            stat.ino(),
            entry.enumerated_kind,
            entry.enumerated_inode,
            entry
                .metadata
                .as_ref()
                .map(|meta| (meta.kind.clone(), meta.device, meta.inode)),
        );
    }
    Ok(())
}

/// A subvolume follows the rule for another volume: its own device and
/// inode, no size or other facts, and it is never entered. The collector keeps
/// it outside the boundary only when the enumerated inode matches the observed
/// one; otherwise it records a per-entry failure.
fn check_subvolume(
    directory: &dyn AnchoredDirectory,
    entries: &[ObservedEntry],
    parent: &Path,
    name: &str,
) -> TestResult {
    let entry = find(entries, name)?;
    let stat = fs::symlink_metadata(parent.join(name))?;
    let meta = entry
        .metadata
        .as_ref()
        .map_err(|error| format!("{name}: {error:?}"))?;
    assert_eq!(meta.kind, ObservedKind::Directory, "{name}");
    assert_ne!(meta.device, fs::metadata(parent)?.dev(), "{name}");
    assert_eq!(
        (meta.device, meta.inode),
        (stat.dev(), stat.ino()),
        "{name}"
    );
    assert!(meta.size.is_err(), "{name}");
    assert!(meta.modified_at.is_none(), "{name}");
    assert!(meta.allocated.is_none(), "{name}");
    assert!(meta.link_count.is_none(), "{name}");
    assert!(meta.space_identity.is_none(), "{name}");
    assert_eq!(
        entry.enumerated_inode, meta.inode,
        "{name}: the collector would record a per-entry failure"
    );
    assert!(directory.open_child(entry).is_err(), "{name} was entered");
    Ok(())
}

impl BtrfsFixture {
    fn exercise(&self) -> TestResult {
        let release = fs::read_to_string("/proc/sys/kernel/osrelease")?;
        eprintln!("F02_LINUX_BTRFS kernel={}", release.trim());
        let mut root = open(&self.mount)?;
        let entries = drain(&mut *root.directory)?;
        let second = self.mount.join("second");
        let mut inner = open(&second)?;
        let nested = drain(&mut *inner.directory)?;
        report(&entries, &self.mount, &["first", "second"])?;
        report(&nested, &second, &["nested"])?;

        let plain = find(&entries, "plain")?;
        let meta = plain
            .metadata
            .as_ref()
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(meta.kind, ObservedKind::Directory);
        assert_eq!(meta.device, fs::metadata(&self.mount)?.dev());
        assert_eq!(meta.inode, plain.enumerated_inode);
        let mut opened = root
            .directory
            .open_child(plain)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(drain(&mut *opened)?.len(), 1);

        for name in ["first", "second"] {
            check_subvolume(&*root.directory, &entries, &self.mount, name)?;
        }
        check_subvolume(&*inner.directory, &nested, &second, "nested")?;
        assert_ne!(root.volume_id, inner.volume_id);
        Ok(())
    }
}

#[test]
#[ignore = "requires an isolated GitHub Linux runner to mount a btrfs loop image with sudo"]
fn btrfs_subvolumes_are_other_volumes_that_are_never_entered() -> TestResult {
    if !is_github_linux_runner() {
        eprintln!("skipping btrfs fixture outside a GitHub Linux runner");
        return Ok(());
    }
    let mut fixture = BtrfsFixture::new()?;
    let result = fixture.exercise();
    let cleanup = fixture.cleanup();
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => {
            Err(format!("{error}; cleanup also failed: {cleanup}").into())
        }
    }
}
