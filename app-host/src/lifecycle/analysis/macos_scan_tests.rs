//! macOS scope review, scan, results and failures through the real adapters.
use super::{
    ipc_calls::{finished, logical, read},
    *,
};
use du_core_contracts::{
    PathBytes,
    analysis_ipc::{AnalysisJobStatus, ScanStart, ScopeReview, SpaceTotal, TargetAccess},
    job::JobState,
    scan_failures::ScanFailures,
};
use du_engine_api::desktop::SharedTableLimit;
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    },
    path::Path,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn draft(path: &Path) -> Value {
    let include = json!({
        "encoding": "unix",
        "raw": PathBytes::from_bytes(path.as_os_str().as_bytes()),
        "display": path.to_string_lossy(),
        "lossy": false
    });
    json!({"includes":[include],"exclusions":[],"boundary":"current_volume","includedMounts":[],"source":"gui"})
}

/// Builds the scope: 135 readable bytes, a zero-byte file, two links and a
/// folder without permissions. Returns whether that folder is actually denied,
/// which does not hold for a superuser.
fn fixture(base: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    fs::create_dir_all(base.join("sub").join("deep"))?;
    fs::create_dir(base.join("empty"))?;
    fs::write(base.join("a.bin"), vec![1u8; 100])?;
    fs::write(base.join("sub").join("b.bin"), vec![2u8; 23])?;
    fs::write(base.join("sub").join("deep").join("c.bin"), vec![3u8; 7])?;
    fs::write(base.join("zero"), b"")?;
    // A decomposed name, kept as the bytes the volume stores.
    fs::write(base.join("e\u{301}.bin"), vec![4u8; 5])?;
    symlink("..", base.join("up"))?;
    symlink("/", base.join("outside"))?;
    let locked = base.join("locked");
    fs::create_dir(&locked)?;
    fs::write(locked.join("hidden"), vec![9u8; 50])?;
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))?;
    Ok(fs::read_dir(&locked).is_err())
}

#[test]
fn macos_scan_reviews_collects_and_reports_failures() -> TestResult {
    let temp = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    // The temporary directory is under /var, a symbolic link to /private/var,
    // and scope roots are opened without following links.
    let base = fs::canonicalize(temp.path())?.join("scope");
    let denied = fixture(&base)?;
    let outcome = scan(&base, profile.path(), denied);
    fs::set_permissions(base.join("locked"), fs::Permissions::from_mode(0o700))?;
    outcome
}

fn scan(base: &Path, profile: &Path, denied: bool) -> TestResult {
    let ports = scan_ports().ok_or("macOS collection adapters are not injected")?;
    let database = crate::startup_database::open(profile)?;
    let mut builder = EngineBuilder::default();
    let limit: SharedTableLimitSource = Arc::new(SharedTableLimit::default);
    let services = register_analysis(&mut builder, ports, &database, limit, None)?;
    let engine = builder.build()?;
    verify_registered(&engine, true)?;
    let alias = base.with_file_name("alias");
    symlink(base, &alias)?;
    let linked: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft":draft(&alias),"requestSeq":1}),
    )?;
    assert_eq!(
        linked.targets[0].reason_key.as_deref(),
        Some("path.unresolvable")
    );
    let draft = draft(base);
    let review: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft":draft,"requestSeq":2}),
    )?;
    assert_eq!(review.targets.len(), 1);
    assert_eq!(review.targets[0].access, TargetAccess::Accessible);
    let start: ScanStart = read(
        &engine,
        "scan.start",
        json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
    )?;
    let finish = finished(&services.events, &start.job_ref)?;
    assert_eq!(finish.body["state"], "done");
    let status: AnalysisJobStatus = read(&engine, "job.get", json!({"jobRef":start.job_ref}))?;
    assert_eq!(status.state, JobState::Done);
    let version = status.result_version.ok_or("missing result version")?;
    let total: SpaceTotal = read(
        &engine,
        "space.total",
        json!({"resultVersion":version,"requestSeq":1,"target":{"kind":"scope_root"}}),
    )?;
    // Untraversed links leave the readable total as a lower bound.
    let (bytes, exact) = logical(&total)?;
    assert_eq!((bytes, exact), (135, false));
    let failures: ScanFailures = read(&engine, "scan.failures", json!({"resultVersion":version}))?;
    let rows: Vec<_> = failures
        .rows
        .iter()
        .map(|row| {
            (
                row.display_path.clone().unwrap_or_default(),
                row.reason_key.clone(),
            )
        })
        .collect();
    let link_rows = rows
        .iter()
        .filter(|(_, reason)| reason == "scan.link_untraversed")
        .count();
    assert_eq!(link_rows, 2, "{rows:?}");
    let denied_rows = rows
        .iter()
        .filter(|(path, reason)| path.ends_with("/locked") && reason == "scan.path_denied")
        .count();
    assert_eq!(denied_rows, usize::from(denied), "{rows:?}");
    assert_eq!(failures.total_count, 2 + u64::from(denied));
    eprintln!(
        "F02_MACOS_ENGINE state=done logical_lower_bound={bytes} completed={} failed={} failure_rows={} denied_case={denied}",
        finish.body["completed"], finish.body["failed"], failures.total_count
    );
    Ok(())
}
