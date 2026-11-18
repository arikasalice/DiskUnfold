//! Reviewed exclusions applied by the real Linux adapter before any metadata read.
use super::{
    trace::{Trace, TracedRoots},
    *,
};
use du_core_contracts::{scan_failures::ScanFailureKind, scan_observation::FilterDecision};

#[test]
fn linux_scan_skips_reviewed_exclusions_before_enumeration() -> TestResult {
    let denied = std::env::var_os("DU_TEST_UNPRIVILEGED").is_some();
    let temp = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let base = temp.path().join("scope");
    fixture(&base, denied)?;
    // A name that differs from the excluded folder only by case.
    fs::create_dir(base.join("Sub"))?;
    fs::write(base.join("Sub").join("d.bin"), vec![5u8; 11])?;
    let outcome = excluded_scan(&base, profile.path(), denied);
    if denied {
        fs::set_permissions(base.join("locked"), fs::Permissions::from_mode(0o700))?;
    }
    outcome
}

fn excluded_scan(base: &Path, profile: &Path, denied: bool) -> TestResult {
    let trace = Arc::new(Trace::default());
    let ports = ScanPorts {
        roots: Arc::new(TracedRoots(trace.clone())),
        volumes: Arc::new(du_platform_os::volume::LinuxVolumes),
    };
    let database = crate::startup_database::open(profile)?;
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(SharedTableLimit::default);
    let services = register_analysis(&mut builder, ports, &database, limit, None)?;
    let engine = builder.build()?;
    let draft = json!({"includes": [unix_path(base)], "exclusions": [
        {"kind": "path", "path": unix_path(&base.join("sub"))},
        {"kind": "path", "path": unix_path(&base.join("locked"))},
        {"kind": "name_pattern", "pattern": "raw-*"}
    ], "boundary": "current_volume", "includedMounts": [], "source": "gui"});
    let review: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    assert_eq!(review.targets.len(), 1);
    assert!(
        !review
            .notices
            .iter()
            .any(|notice| notice == "scope.exclusion_ineffective"),
        "{:?}",
        review.notices
    );
    let start: ScanStart = read(
        &engine,
        "scan.start",
        json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
    )?;
    let finish = finished(&services.events, &start.job_ref)?;
    assert_eq!(finish.body["state"], "done");
    let status: AnalysisJobStatus = read(&engine, "job.get", json!({"jobRef": start.job_ref}))?;
    let version = status.result_version.ok_or("missing result version")?;
    let total: SpaceTotal = read(
        &engine,
        "space.total",
        json!({"resultVersion": version, "requestSeq": 1, "target": {"kind": "scope_root"}}),
    )?;
    // 135 readable fixture bytes, less sub (30) and raw-\xff (5), plus Sub/d.bin (11) and
    // the links' own 3 bytes. The excluded denied folder is never opened, so it is exact.
    assert_eq!(logical(&total)?, (114, true));
    let failures: ScanFailures = read(&engine, "scan.failures", json!({"resultVersion": version}))?;
    // Only the untraversed links remain; an excluded unreadable folder is never opened.
    let rows: Vec<_> = failures
        .rows
        .iter()
        .map(|row| (row.kind, row.display_path.clone().unwrap_or_default()))
        .collect();
    assert_eq!(failures.total_count, 2, "{rows:?}");
    assert!(
        rows.iter()
            .all(|(kind, _)| *kind == ScanFailureKind::UntraversedLink),
        "{rows:?}"
    );
    assert_eq!(trace.decisions(b"sub")?, [FilterDecision::Exclude]);
    assert_eq!(trace.decisions(b"raw-\xff")?, [FilterDecision::Exclude]);
    assert_eq!(
        trace.decisions(b"locked")?,
        if denied {
            vec![FilterDecision::Exclude]
        } else {
            Vec::new()
        }
    );
    assert_eq!(trace.decisions(b"Sub")?, [FilterDecision::Admit]);
    assert_eq!(trace.decisions(b"d.bin")?, [FilterDecision::Admit]);
    // The excluded folders were never opened, so none of their names were enumerated.
    for hidden in [&b"deep"[..], b"b.bin", b"c.bin", b"hidden"] {
        assert!(trace.decisions(hidden)?.is_empty(), "{hidden:?}");
    }
    let offered = trace.offered.lock().map_err(|_| "trace poisoned")?.len();
    let opened = trace.opened.lock().map_err(|_| "trace poisoned")?.clone();
    assert!(
        !opened
            .iter()
            .any(|name| name == b"sub" || name == b"locked"),
        "{opened:?}"
    );
    assert!(opened.iter().any(|name| name == b"Sub"), "{opened:?}");
    eprintln!(
        "F02_LINUX_EXCLUSION state=done logical_bytes=114 offered={offered} opened={} failure_rows={} denied_case={denied}",
        opened.len(),
        failures.total_count
    );
    Ok(())
}
