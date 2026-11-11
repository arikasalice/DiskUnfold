//! Real Windows scope review, scan admission, and pinned totals for multiple roots.
use super::*;
use du_core_contracts::{
    SizeAmount, SizeBasis,
    analysis_ipc::{AnalysisJobStatus, SpaceTotal},
    result::ResultCompleteness,
};
use std::{collections::BTreeSet, fs};

const ROOTS: usize = 65;

fn read<T: serde::de::DeserializeOwned>(
    engine: &Engine,
    name: &str,
    body: Value,
) -> Result<T, Box<dyn std::error::Error>> {
    let response = ipc(engine, name, body)?;
    if let Some(error) = response.error {
        return Err(format!("{name}: {error:?}").into());
    }
    Ok(serde_json::from_value(response.body)?)
}

fn logical(total: &SpaceTotal) -> Result<u64, Box<dyn std::error::Error>> {
    let amount = &total
        .sizes
        .iter()
        .find(|value| value.size.basis == SizeBasis::Logical)
        .ok_or("missing logical size")?
        .size
        .amount;
    match amount {
        SizeAmount::Exact { bytes } => Ok(bytes.as_str().parse()?),
        _ => Err(format!("logical size is not exact: {amount:?}").into()),
    }
}

#[test]
fn windows_sixty_five_roots_pass_desktop_admission_and_keep_exact_pinned_totals()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let mut includes = Vec::new();
    for index in 0..ROOTS {
        let dir = fixture.path().join(format!("root-{index:03}"));
        fs::create_dir(&dir)?;
        fs::write(
            dir.join(format!("unique-{index:03}.bin")),
            vec![index as u8; index + 1],
        )?;
        let raw: Vec<_> = dir
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        includes.push(json!({"encoding":"windows_utf16le","raw":PathBytes::from_bytes(&raw),"display":dir,"lossy":false}));
    }
    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let diagnostics = Arc::new(du_engine_api::desktop::ScanDiagnostics::default());
    let diagnostic_factory = diagnostics.clone();
    let events = du_engine_api::desktop::register_with_scan_diagnostics(
        &mut builder,
        Arc::new(du_platform_os::windows_observation::WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        &du_platform_os::entropy::OsRandom,
        Arc::new(move || diagnostic_factory.clone()),
    )?;
    let engine = builder.build()?;
    let draft = json!({"includes":includes,"exclusions":[],"boundary":"current_volume","includedMounts":[],"source":"gui"});
    let review: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft":draft,"requestSeq":1}),
    )?;
    assert_eq!(review.scope.includes.len(), ROOTS);
    assert_eq!(review.targets.len(), ROOTS);
    let start: ScanStart = read(
        &engine,
        "scan.start",
        json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
    )?;
    assert!(!start.existing);
    let events = terminal_batch(&events, &start.job_ref)?;
    let finish = events
        .iter()
        .find(|event| {
            event.topic == "job.finished" && event.job_ref.as_ref() == Some(&start.job_ref)
        })
        .ok_or("missing finish")?;
    assert_eq!(finish.body["state"], "done");
    assert_eq!(finish.body["completed"], 65);
    for key in ["failed", "skipped", "unprocessed"] {
        assert_eq!(finish.body[key], 0, "{key}");
    }
    let status: AnalysisJobStatus = read(&engine, "job.get", json!({"jobRef":start.job_ref}))?;
    assert_eq!(status.state, JobState::Done);
    let terminal = status.terminal.as_ref().ok_or("missing terminal")?;
    assert_eq!(terminal.completed, ROOTS as u64);
    assert_eq!(
        (terminal.failed, terminal.skipped, terminal.unprocessed),
        (0, 0, 0)
    );
    let result = status.scan_result.ok_or("missing result lineage")?;
    assert_eq!(Some(&result.lineage_id), start.lineage_id.as_ref());
    let version = result.result_version.ok_or("missing result version")?;
    assert_eq!(status.result_version.as_ref(), Some(&version));
    verify_pinned_totals(&engine, &version)?;
    eprintln!(
        "F02_WINDOWS_ROOTS reviewed=65 completed=65 total_bytes=2145 unique_root_totals={ROOTS} shared_ready_peak={:?}",
        diagnostics.shared_ready_peak()
    );
    Ok(())
}

fn verify_pinned_totals(engine: &Engine, version: &Id) -> Result<(), Box<dyn std::error::Error>> {
    let total: SpaceTotal = read(
        engine,
        "space.total",
        json!({"resultVersion":version,"requestSeq":1,"target":{"kind":"scope_root"}}),
    )?;
    assert_eq!(&total.result_version, version);
    assert_eq!(total.completeness, ResultCompleteness::Complete);
    assert_eq!(logical(&total)?, 2145);
    let mut sizes = BTreeSet::new();
    for item in 0..ROOTS {
        let per_root: SpaceTotal = read(
            engine,
            "space.total",
            json!({"resultVersion":version,"requestSeq":item+2,"target":{"kind":"folder","item":{"resultVersion":version,"itemNo":item}}}),
        )?;
        assert_eq!(&per_root.result_version, version);
        assert_eq!(per_root.completeness, ResultCompleteness::Complete);
        let bytes = logical(&per_root)?;
        assert_eq!(bytes, item as u64 + 1, "unexpected size for root {item}");
        assert!(sizes.insert(bytes), "duplicate root size");
    }
    assert_eq!(sizes, (1..=65).collect());
    let repeat: SpaceTotal = read(
        engine,
        "space.total",
        json!({"resultVersion":version,"requestSeq":100,"target":{"kind":"scope_root"}}),
    )?;
    assert_eq!(logical(&repeat)?, 2145);
    Ok(())
}

/// Uses isolated fixtures on two mounted NTFS volumes and checks one combined result.
#[test]
#[ignore = "requires DISKUNFOLD_TEST_SECOND_NTFS_PARENT on another writable NTFS volume"]
fn windows_two_ntfs_volumes_keep_root_and_scope_totals_on_one_result()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::analysis_ipc::{IdentitySource, TargetList};
    use du_core_contracts::scan_observation::ScopeRootPort;
    use du_platform_os::windows_observation::WindowsScopeRoots;

    let second_parent = std::env::var_os("DISKUNFOLD_TEST_SECOND_NTFS_PARENT")
        .ok_or("set DISKUNFOLD_TEST_SECOND_NTFS_PARENT to a directory on the second volume")?;
    let first = tempfile::tempdir()?;
    let second = tempfile::tempdir_in(second_parent)?;
    let profile = tempfile::tempdir()?;
    let mut includes = Vec::new();
    let mut volumes = BTreeSet::new();
    for (root, bytes) in [(first.path(), 13), (second.path(), 37)] {
        fs::write(root.join("size.bin"), vec![0_u8; bytes])?;
        let raw: Vec<_> = root
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        let opened = WindowsScopeRoots
            .open_root(&raw)
            .map_err(|error| format!("{error:?}"))?;
        volumes.insert(PathBytes::from_bytes(&opened.volume_id).as_str().to_owned());
        includes.push(json!({"encoding":"windows_utf16le","raw":PathBytes::from_bytes(&raw),"display":root,"lossy":false}));
    }
    assert_eq!(
        volumes.len(),
        2,
        "fixture roots must have distinct native volume identities"
    );

    let database = crate::startup_database::open(profile.path())?;
    let mut builder = EngineBuilder::default();
    let events = du_engine_api::desktop::register(
        &mut builder,
        Arc::new(WindowsScopeRoots),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        &du_platform_os::entropy::OsRandom,
    )?;
    let engine = builder.build()?;
    let targets: TargetList = read(&engine, "target.list", json!({"refresh":false}))?;
    assert_eq!(targets.unavailable_reason, None);
    for volume in &volumes {
        assert!(
            targets.targets.iter().any(|target| {
                target.connected
                    && target.identity_source == IdentitySource::Confirmed
                    && target.volume_id.as_ref() == Some(volume)
            }),
            "target discovery omitted a selected volume"
        );
    }
    let draft = json!({"includes":includes,"exclusions":[],"boundary":"current_volume","includedMounts":[],"source":"gui"});
    let review: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft":draft,"requestSeq":1}),
    )?;
    assert_eq!(review.scope.includes.len(), 2);
    assert_eq!(review.targets.len(), 2);
    assert!(
        review.targets.iter().all(
            |target| target.access == du_core_contracts::analysis_ipc::TargetAccess::Accessible
        )
    );
    let start: ScanStart = read(
        &engine,
        "scan.start",
        json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
    )?;
    assert!(!start.existing);
    let events = terminal_batch(&events, &start.job_ref)?;
    let finish = events
        .iter()
        .find(|event| {
            event.topic == "job.finished" && event.job_ref.as_ref() == Some(&start.job_ref)
        })
        .ok_or("missing scan finish")?;
    assert_eq!(finish.body["state"], "done");
    for key in ["failed", "skipped", "unprocessed"] {
        assert_eq!(finish.body[key], 0, "{key}");
    }
    let version = finish
        .result_version
        .as_ref()
        .ok_or("missing result version")?;
    let mut root_totals = BTreeSet::new();
    for (seq, target) in [
        json!({"kind":"scope_root"}),
        json!({"kind":"folder","item":{"resultVersion":version,"itemNo":0}}),
        json!({"kind":"folder","item":{"resultVersion":version,"itemNo":1}}),
    ]
    .into_iter()
    .enumerate()
    {
        let total: SpaceTotal = read(
            &engine,
            "space.total",
            json!({"resultVersion":version,"requestSeq":seq,"target":target}),
        )?;
        assert_eq!(&total.result_version, version);
        assert_eq!(total.request_seq as usize, seq);
        assert_eq!(total.completeness, ResultCompleteness::Complete);
        if seq == 0 {
            assert_eq!(logical(&total)?, 50);
        } else {
            root_totals.insert(logical(&total)?);
        }
    }
    assert_eq!(root_totals, BTreeSet::from([13, 37]));
    eprintln!("F01_F03_WINDOWS_VOLUMES discovered=2 distinct=2 root_bytes=13,37 scope_bytes=50");
    Ok(())
}
