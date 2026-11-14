//! Mount points below a scanned root, observed through the real Linux adapter.
use super::{
    trace::{Trace, TracedRoots},
    *,
};
use du_core_contracts::{scan_failures::ScanFailureKind, scan_observation::FilterDecision};
use std::os::unix::fs::MetadataExt;

/// Scans `/` with every top-level name excluded except the system mounts.
///
/// Each mount point is classified outside the boundary: it is recorded as an
/// unread range, never opened or enumerated, and not counted as a failure.
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
    for name in &mounts {
        let suffix = format!("/{}", String::from_utf8_lossy(name));
        assert!(
            rows.iter()
                .any(|(kind, reason, path)| *kind == ScanFailureKind::UnreadRange
                    && reason == "scan.unclassified"
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
        "F02_LINUX_BOUNDARY state=done failed=0 outside_boundary={} offered={offered} opened=0",
        mounts.len()
    );
    Ok(())
}
