//! Startup abort mapping of the size rule registration (F03 FR14.HD2.app-host.DD3).
use super::*;
use du_engine_api::size_rule::{DEFAULT_SIZE_RULE_ID, default_size_rule_source};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn default_bundle_assembles_once() -> TestResult {
    let rules = assemble(default_size_rule_source())?;
    let identities = rules.identities();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].id, DEFAULT_SIZE_RULE_ID);
    assert_eq!(identities[0].revision, 1);
    Ok(())
}

#[test]
fn missing_default_rule_stops_startup() -> TestResult {
    let error = assemble_from(SizeRuleRegistryBuilder::new(), default_size_rule_source())
        .err()
        .ok_or("an empty registry was accepted")?;
    assert_eq!(
        error.to_string(),
        "startup.registration_failed: default size rule not registered"
    );
    Ok(())
}

#[test]
fn duplicate_registration_stops_startup() -> TestResult {
    let mut builder = SizeRuleRegistryBuilder::new();
    register_default_size_rules(&mut builder)?;
    let error = register_default_size_rules(&mut builder)
        .map_err(registration_failed)
        .err()
        .ok_or("a second registration was accepted")?;
    assert_eq!(
        error.to_string(),
        "startup.registration_failed: size rule registered twice"
    );
    Ok(())
}
