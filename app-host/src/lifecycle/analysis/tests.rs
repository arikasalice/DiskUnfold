//! Assembly checks for the scan registration (FR1.HD2.app-host.DD1).
use super::*;
use du_engine_api::desktop::SharedTableLimit;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn missing_scan_use_cases_stop_startup_once_analysis_is_registered() -> TestResult {
    let engine = EngineBuilder::default().build()?;
    let error = verify_registered(&engine, true)
        .err()
        .ok_or("missing scan use cases were accepted")?;
    assert_eq!(
        error.to_string(),
        "startup.registration_failed: missing use case scan.start"
    );
    assert!(verify_registered(&engine, false).is_ok());
    Ok(())
}

#[test]
fn platform_adapters_register_scanning_once() -> TestResult {
    let Some(ports) = scan_ports() else {
        // This platform does not supply collection adapters yet.
        return Ok(());
    };
    let profile = tempfile::tempdir()?;
    let database = crate::startup_database::open(profile.path())?;
    let limit: SharedTableLimitSource = Arc::new(SharedTableLimit::default);
    let mut builder = EngineBuilder::default();
    register_analysis(&mut builder, ports.clone(), &database, limit.clone(), None)?;
    let engine = builder.build()?;
    verify_registered(&engine, true)?;
    assert!(engine.has_use_case("scope.review"));
    let mut twice = EngineBuilder::default();
    register_analysis(&mut twice, ports.clone(), &database, limit.clone(), None)?;
    let rejected = register_analysis(&mut twice, ports, &database, limit, None).is_err();
    assert!(rejected || twice.build().is_err());
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_supplies_its_own_collection_adapters() {
    assert!(scan_ports().is_some());
}
