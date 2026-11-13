//! Evidence-based Full Disk Access (FDA) probe for the macOS startup adapter.
//!
//! macOS has no supported API that reports the grant directly. The only direct
//! evidence is the kernel/TCC answer to a read-only open of a path that the
//! system protects with FDA. `EPERM` or `EACCES` there is the OS refusing this
//! process; a successful open and read is the OS admitting it. Everything else
//! (missing path, unusual errno, no home directory) proves nothing.
//!
//! The probe never writes, never creates files and never prompts the user. It
//! observes the grant of the calling process (and its responsible app), which
//! is the access the analysis engine will actually have.
//! Probing uses only safe standard-library I/O. The one unsafe call, the
//! passwd lookup of the home directory, lives in the `passwd` submodule.

use super::FullDiskAccessStatus;
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod passwd;

/// What one read-only attempt on a protected path established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProbeEvidence {
    /// The path was opened and read; the OS admitted this process.
    Readable,
    /// The OS refused with `EPERM` or `EACCES`.
    Blocked,
    /// The path does not exist (`ENOENT`/`ENOTDIR`); no inference is possible.
    Absent,
    /// Any other outcome; no inference is possible.
    Inconclusive,
}

/// Protected locations, relative to the user's home directory.
const USER_PROBES: [&str; 2] = [
    "Library/Application Support/com.apple.TCC/TCC.db",
    "Library/Safari",
];

/// Protected location that does not depend on the home directory.
const SYSTEM_PROBE: &str = "/Library/Application Support/com.apple.TCC/TCC.db";

/// Maps an I/O error to the evidence it provides.
pub(super) fn evidence_from_error(error: &io::Error) -> ProbeEvidence {
    match error.kind() {
        io::ErrorKind::PermissionDenied => ProbeEvidence::Blocked,
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => ProbeEvidence::Absent,
        _ => match error.raw_os_error() {
            Some(1 | 13) => ProbeEvidence::Blocked,
            _ => ProbeEvidence::Inconclusive,
        },
    }
}

/// Opens `path` read-only and reads at most one byte (or one directory entry).
pub(super) fn probe_path(path: &Path) -> ProbeEvidence {
    let outcome = match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::read_dir(path).and_then(first_entry_outcome),
        Ok(_) => fs::File::open(path).and_then(|mut file| file.read(&mut [0u8; 1]).map(|_| ())),
        Err(error) => Err(error),
    };
    match outcome {
        Ok(()) => ProbeEvidence::Readable,
        Err(error) => evidence_from_error(&error),
    }
}

/// Reads at most one directory entry. The first `readdir` can fail with
/// `EPERM` after `opendir` succeeded, and that refusal must not count as a read.
pub(super) fn first_entry_outcome<T>(
    mut entries: impl Iterator<Item = io::Result<T>>,
) -> io::Result<()> {
    match entries.next() {
        Some(Err(error)) => Err(error),
        Some(Ok(_)) | None => Ok(()),
    }
}

/// Combines per-path evidence with fail-closed semantics.
///
/// `Granted` requires at least one readable protected path and no refusal;
/// contradictory evidence is `Unknown`. `Denied` requires a refusal and no
/// readable path. Missing or inconclusive paths never decide the outcome.
pub(super) fn aggregate(evidence: &[ProbeEvidence]) -> FullDiskAccessStatus {
    let readable = evidence.contains(&ProbeEvidence::Readable);
    let blocked = evidence.contains(&ProbeEvidence::Blocked);
    match (readable, blocked) {
        (true, false) => FullDiskAccessStatus::Granted,
        (false, true) => FullDiskAccessStatus::Denied,
        _ => FullDiskAccessStatus::Unknown,
    }
}

/// Runs the probe set through `probe`, which is injected so tests need no OS.
pub(super) fn full_disk_access_with(
    home: Option<PathBuf>,
    probe: impl Fn(&Path) -> ProbeEvidence,
) -> FullDiskAccessStatus {
    let mut evidence: Vec<ProbeEvidence> = Vec::with_capacity(3);
    if let Some(home) = home.filter(|home| home.is_absolute()) {
        evidence.extend(
            USER_PROBES
                .iter()
                .map(|relative| probe(&home.join(relative))),
        );
    }
    evidence.push(probe(Path::new(SYSTEM_PROBE)));
    aggregate(&evidence)
}

/// Production probe using the home directory from the passwd entry of the
/// real user, which an inherited `$HOME` cannot redirect.
#[cfg(target_os = "macos")]
pub(super) fn full_disk_access_status() -> FullDiskAccessStatus {
    full_disk_access_with(passwd::home_directory(), probe_path)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::ProbeEvidence::{Absent, Blocked, Inconclusive, Readable};
    use super::*;

    fn run(results: [ProbeEvidence; 3]) -> FullDiskAccessStatus {
        let home = std::env::temp_dir();
        let paths = [
            home.join("Library/Application Support/com.apple.TCC/TCC.db"),
            home.join("Library/Safari"),
            PathBuf::from(SYSTEM_PROBE),
        ];
        full_disk_access_with(Some(home), |path| {
            let index = paths.iter().position(|candidate| candidate == path);
            results[index.expect("unexpected probe path")]
        })
    }

    #[test]
    fn readable_protected_path_without_refusal_is_granted() {
        assert_eq!(
            run([Readable, Absent, Readable]),
            FullDiskAccessStatus::Granted
        );
    }

    #[test]
    fn refusal_without_readable_path_is_denied() {
        assert_eq!(
            run([Blocked, Blocked, Blocked]),
            FullDiskAccessStatus::Denied
        );
        assert_eq!(
            run([Absent, Blocked, Inconclusive]),
            FullDiskAccessStatus::Denied
        );
    }

    #[test]
    fn missing_or_inconclusive_evidence_is_unknown() {
        assert_eq!(run([Absent, Absent, Absent]), FullDiskAccessStatus::Unknown);
        assert_eq!(
            run([Inconclusive, Absent, Inconclusive]),
            FullDiskAccessStatus::Unknown
        );
    }

    #[test]
    fn contradictory_evidence_never_reports_granted() {
        assert_eq!(
            run([Readable, Blocked, Absent]),
            FullDiskAccessStatus::Unknown
        );
    }

    #[test]
    fn missing_or_relative_home_probes_only_the_system_path() {
        for home in [None, Some(PathBuf::from("relative"))] {
            let seen = std::cell::RefCell::new(Vec::new());
            let status = full_disk_access_with(home, |path| {
                seen.borrow_mut().push(path.to_path_buf());
                Blocked
            });
            assert_eq!(status, FullDiskAccessStatus::Denied);
            assert_eq!(*seen.borrow(), vec![PathBuf::from(SYSTEM_PROBE)]);
        }
    }

    #[test]
    fn a_refused_first_readdir_is_blocked_not_readable() {
        let outcome = |entry: io::Result<()>| {
            first_entry_outcome(std::iter::once(entry))
                .map_or_else(|error| evidence_from_error(&error), |()| Readable)
        };
        assert_eq!(
            outcome(Err(io::Error::from(io::ErrorKind::PermissionDenied))),
            Blocked
        );
        #[cfg(unix)]
        assert_eq!(outcome(Err(io::Error::from_raw_os_error(1))), Blocked);
        assert_eq!(outcome(Ok(())), Readable);
        assert!(first_entry_outcome(std::iter::empty::<io::Result<()>>()).is_ok());
    }

    #[test]
    fn errno_mapping_separates_refusal_from_absence() {
        #[cfg(unix)]
        {
            let code = |errno| evidence_from_error(&io::Error::from_raw_os_error(errno));
            assert_eq!(code(1), Blocked); // EPERM
            assert_eq!(code(13), Blocked); // EACCES
            assert_eq!(code(2), Absent); // ENOENT
            assert_eq!(code(5), Inconclusive); // EIO
        }
        let kind = |kind| evidence_from_error(&io::Error::from(kind));
        assert_eq!(kind(io::ErrorKind::PermissionDenied), Blocked);
        assert_eq!(kind(io::ErrorKind::NotFound), Absent);
        assert_eq!(
            evidence_from_error(&io::Error::other("opaque")),
            Inconclusive
        );
    }

    #[test]
    fn probe_path_reads_files_and_directories_and_reports_absence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("f");
        fs::write(&file, b"x").expect("write");
        assert_eq!(probe_path(&file), Readable);
        assert_eq!(probe_path(dir.path()), Readable);
        assert_eq!(probe_path(&dir.path().join("missing")), Absent);
    }
}
