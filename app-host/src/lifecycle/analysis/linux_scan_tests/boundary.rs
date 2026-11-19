//! Mount points below a scanned root, observed through the real Linux adapter.
use super::{
    trace::{Trace, TracedRoots},
    *,
};
use du_core_contracts::{
    result::ResultCompleteness,
    scan_failures::{ScanFailureKind, unentered_reason::BOUNDARY_OTHER_VOLUME},
    scan_observation::FilterDecision,
};
use std::os::unix::fs::MetadataExt;

/// Scans `/` with every top-level name excluded except the system mounts.
///
/// The adapter reports each mount point as `ObservedKind::MountPoint`, so it is
/// classified outside the boundary: listed as an unread range, never opened or
/// enumerated, not counted as a failure and not a scope gap. The result stays
/// complete.
#[test]
fn linux_scan_leaves_system_mounts_outside_the_boundary() -> TestResult {
    let root_device = fs::metadata("/")?.dev();
    let mut others = Vec::new();
    let mut mounts = Vec::new();
    for entry in fs::read_dir("/")? {
        let entry = entry?;
        let name = entry.file_name().as_bytes().to_vec();
        let system = [&b"proc"[..], b"sys", b"dev"].contains(&name.as_slice());
        let mounted = fs::symlink_metadata(entry.path())
            .is_ok_and(|meta| meta.is_dir() && meta.dev() != root_device);
        if system && mounted {
            mounts.push(name);
        } else {
            others.push(entry.path());
        }
    }
    if mounts.is_empty() {
        eprintln!(
            "F02_LINUX_BOUNDARY no separate system mount below /; boundary case not observed"
        );
        return Ok(());
    }
    let profile = tempfile::tempdir()?;
    let trace = Arc::new(Trace::default());
    let ports = ScanPorts {
        roots: Arc::new(TracedRoots(trace.clone())),
        volumes: Arc::new(du_platform_os::volume::LinuxVolumes),
    };
    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(SharedTableLimit::default);
    let services = register_analysis(&mut builder, ports, &database, limit, None)?;
    let engine = builder.build()?;
    let exclusions: Vec<_> = others
        .iter()
        .map(|path| json!({"kind": "path", "path": unix_path(path)}))
        .collect();
    let draft = json!({"includes": [unix_path(Path::new("/"))], "exclusions": exclusions,
        "boundary": "current_volume", "includedMounts": [], "source": "gui"});
    let review: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    let start: ScanStart = read(
        &engine,
        "scan.start",
        json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
    )?;
    let finish = finished(&services.events, &start.job_ref)?;
    assert_eq!(finish.body["state"], "done", "{}", finish.body);
    assert_eq!(finish.body["failed"], 0, "{}", finish.body);
    let status: AnalysisJobStatus = read(&engine, "job.get", json!({"jobRef": start.job_ref}))?;
    let version = status.result_version.ok_or("missing result version")?;
    let total: SpaceTotal = read(
        &engine,
        "space.total",
        json!({"resultVersion": version, "requestSeq": 1, "target": {"kind": "scope_root"}}),
    )?;
    assert_eq!(total.completeness, ResultCompleteness::Complete);
    assert_eq!(total.inaccessible_ranges, None);
    let failures: ScanFailures = read(&engine, "scan.failures", json!({"resultVersion": version}))?;
    let rows: Vec<_> = failures
        .rows
        .iter()
        .map(|row| {
            (
                row.kind,
                row.reason_key.clone(),
                row.display_path.clone().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(rows.len(), mounts.len(), "{rows:?}");
    assert_eq!(failures.total_count, mounts.len() as u64);
    for name in &mounts {
        let suffix = format!("/{}", String::from_utf8_lossy(name));
        assert!(
            rows.iter()
                .any(|(kind, reason, path)| *kind == ScanFailureKind::UnreadRange
                    && reason == BOUNDARY_OTHER_VOLUME
                    && path.ends_with(&suffix)),
            "{suffix}: {rows:?}"
        );
        assert_eq!(trace.decisions(name)?, [FilterDecision::Admit]);
    }
    let opened = trace.opened.lock().map_err(|_| "trace poisoned")?.clone();
    assert!(opened.is_empty(), "{opened:?}");
    // Only the names of `/` were offered; nothing below a mount was enumerated.
    let offered = trace.offered.lock().map_err(|_| "trace poisoned")?.len();
    assert_eq!(offered, mounts.len() + others.len());
    eprintln!(
        "F02_LINUX_BOUNDARY state=done failed=0 complete outside_boundary={} offered={offered} opened=0",
        mounts.len()
    );
    Ok(())
}

/// What one scan of a root reported through the GUI use cases.
struct Scanned {
    failed: Value,
    completeness: ResultCompleteness,
    logical: (u64, bool),
    rows: Vec<(ScanFailureKind, String, String)>,
}

/// Scans `root` with the real Linux adapters and no exclusion.
fn scan_root(root: &Path) -> Result<Scanned, Box<dyn std::error::Error>> {
    let profile = tempfile::tempdir()?;
    let ports = scan_ports().ok_or("Linux collection adapters are not injected")?;
    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(SharedTableLimit::default);
    let services = register_analysis(&mut builder, ports, &database, limit, None)?;
    let engine = builder.build()?;
    let draft = json!({"includes": [unix_path(root)], "exclusions": [],
        "boundary": "current_volume", "includedMounts": [], "source": "gui"});
    let review: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    let start: ScanStart = read(
        &engine,
        "scan.start",
        json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
    )?;
    let finish = finished(&services.events, &start.job_ref)?;
    assert_eq!(finish.body["state"], "done", "{}", finish.body);
    let status: AnalysisJobStatus = read(&engine, "job.get", json!({"jobRef": start.job_ref}))?;
    let version = status.result_version.ok_or("missing result version")?;
    let total: SpaceTotal = read(
        &engine,
        "space.total",
        json!({"resultVersion": version, "requestSeq": 1, "target": {"kind": "scope_root"}}),
    )?;
    let failures: ScanFailures = read(&engine, "scan.failures", json!({"resultVersion": version}))?;
    Ok(Scanned {
        failed: finish.body["failed"].clone(),
        completeness: total.completeness,
        logical: logical(&total)?,
        rows: failures
            .rows
            .iter()
            .map(|row| {
                (
                    row.kind,
                    row.reason_key.clone(),
                    row.display_path.clone().unwrap_or_default(),
                )
            })
            .collect(),
    })
}

/// Scans the btrfs volume that the Linux CI step mounts at
/// `$DU_TEST_BTRFS_ROOT`: the subvolumes `first`, `second` and
/// `second/nested` (IDs 256, 257 and 258), 10 bytes in `plain` and 5 bytes
/// in `second`. Each subvolume below a scanned root is one boundary row and
/// never a failure, and the result stays complete.
#[test]
#[ignore = "requires the btrfs volume that the Linux CI step mounts"]
fn linux_scan_leaves_btrfs_subvolumes_outside_the_boundary() -> TestResult {
    let Some(root) = std::env::var_os("DU_TEST_BTRFS_ROOT") else {
        eprintln!("skipping: DU_TEST_BTRFS_ROOT is not set");
        return Ok(());
    };
    let root = Path::new(&root);
    let scopes = [
        (root.to_path_buf(), &["first", "second"][..], 10),
        (root.join("second"), &["nested"][..], 5),
    ];
    for (scope, names, bytes) in scopes {
        let scanned = scan_root(&scope)?;
        assert_eq!(scanned.failed, 0, "{}: {:?}", scope.display(), scanned.rows);
        assert_eq!(scanned.completeness, ResultCompleteness::Complete);
        assert_eq!(scanned.logical, (bytes, true), "{}", scope.display());
        assert_eq!(scanned.rows.len(), names.len(), "{:?}", scanned.rows);
        for name in names {
            let suffix = format!("/{name}");
            assert!(
                scanned.rows.iter().any(|(kind, reason, path)| {
                    *kind == ScanFailureKind::UnreadRange
                        && reason == BOUNDARY_OTHER_VOLUME
                        && path.ends_with(&suffix)
                }),
                "{suffix}: {:?}",
                scanned.rows
            );
        }
        eprintln!(
            "F02_LINUX_BTRFS_SCAN scope={} state=done failed=0 complete boundary_rows={} logical_bytes={bytes}",
            scope.display(),
            names.len()
        );
    }
    Ok(())
}
