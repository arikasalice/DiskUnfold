//! F02 #218: a real Windows scan without exclusion rules records the correlated evidence
//! of every folder attempt in its result build (F02 DLD IF-19).
use super::*;
use std::fs;

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

#[test]
fn windows_scan_without_exclusions_ends_an_evidence_attempt_for_every_folder()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let profile = tempfile::tempdir()?;
    let root = fixture.path().join("scope");
    // Four folders: the root, a, a\a1 and b.
    fs::create_dir_all(root.join("a").join("a1"))?;
    fs::create_dir_all(root.join("b"))?;
    fs::write(root.join("top.bin"), [1_u8; 3])?;
    fs::write(root.join("a").join("a1").join("deep.bin"), [2_u8; 5])?;
    fs::write(root.join("b").join("side.bin"), [3_u8; 7])?;
    let raw: Vec<_> = root
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let include = json!({"encoding":"windows_utf16le","raw":PathBytes::from_bytes(&raw),"display":root,"lossy":false});
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
    let draft = json!({"includes":[include],"exclusions":[],"boundary":"current_volume","includedMounts":[],"source":"gui"});
    let review: ScopeReview = read(
        &engine,
        "scope.review",
        json!({"draft":draft,"requestSeq":1}),
    )?;
    let start: ScanStart = read(
        &engine,
        "scan.start",
        json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
    )?;
    let events = terminal_batch(&events, &start.job_ref)?;
    let finish = events
        .iter()
        .find(|event| {
            event.topic == "job.finished" && event.job_ref.as_ref() == Some(&start.job_ref)
        })
        .ok_or("missing finish")?;
    assert_eq!(finish.body["state"], "done");
    assert_eq!(diagnostics.ended_folder_attempts(), 4);
    eprintln!(
        "F02_WINDOWS_ATTEMPT_EVIDENCE folders=4 ended_attempts={}",
        diagnostics.ended_folder_attempts()
    );
    Ok(())
}
