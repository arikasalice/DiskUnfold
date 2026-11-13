use std::error::Error;

use serde_json::Value;

use super::*;

fn spec(basis: Option<BasisSpec>) -> SupportTableSpec {
    SupportTableSpec {
        as_of: "2026-11-13".into(),
        groups: vec![GroupSpec {
            os: "windows".into(),
            releases: vec![ReleaseSpec {
                release: "11-25H2".into(),
                security_basis: basis,
            }],
            editions: vec!["Professional".into(), "Core".into()],
            cpus: vec!["x64".into(), "arm64".into()],
        }],
    }
}

fn basis(confirmed: bool) -> BasisSpec {
    BasisSpec {
        kind: Some("general".into()),
        source: Some("https://example.invalid/lifecycle".into()),
        checked_on: Some("2026-11-13".into()),
        confirmed,
        vendor_end_date: None,
    }
}

fn record(id: &str, checks: &[(&str, &str)]) -> TestRecord {
    TestRecord {
        id: id.into(),
        os: "windows".into(),
        release: "11-25H2".into(),
        edition: "Professional".into(),
        cpu: "x64".into(),
        observed_on: "2026-11-13".into(),
        os_detail: "Windows 11 25H2 Pro".into(),
        device: "workstation".into(),
        execution_form: "portable exe".into(),
        checks: checks
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        limits: vec!["installed start not run".into()],
        evidence: "test log".into(),
        features: Vec::new(),
    }
}

fn status_of(table: &SupportTable, edition: &str, cpu: &str) -> Option<&'static str> {
    table
        .rows
        .iter()
        .find(|r| r.edition == edition && r.cpu == cpu)
        .map(|r| r.status)
}

#[test]
fn rows_without_evidence_are_not_verified_and_never_borrow_a_neighbour() {
    let records = [record(
        "r1",
        &[
            ("native_build", "passed"),
            ("native_tests", "passed"),
            ("installed_start", "not_run"),
        ],
    )];
    let table = build(&spec(Some(basis(true))), &records, Mode::Release);
    assert_eq!(table.rows.len(), 4);
    assert_eq!(
        status_of(&table, "Professional", "x64"),
        Some("conditional")
    );
    for (edition, cpu) in [
        ("Professional", "arm64"),
        ("Core", "x64"),
        ("Core", "arm64"),
    ] {
        assert_eq!(status_of(&table, edition, cpu), Some("not_verified"));
        let row = table
            .rows
            .iter()
            .find(|r| r.edition == edition && r.cpu == cpu);
        assert!(row.is_some_and(|r| r.records.is_empty() && r.details.is_empty()));
    }
    assert!(table.blockers.is_empty());
}

#[test]
fn official_verification_needs_every_required_check_to_pass() {
    let all = [
        ("native_build", "passed"),
        ("native_tests", "passed"),
        ("installed_start", "passed"),
    ];
    let table = build(
        &spec(Some(basis(true))),
        &[record("r1", &all)],
        Mode::Release,
    );
    assert_eq!(
        status_of(&table, "Professional", "x64"),
        Some("officially_verified")
    );
    let limited = [
        ("native_build", "passed"),
        ("native_tests", "passed"),
        ("installed_start", "limited"),
    ];
    let table = build(
        &spec(Some(basis(true))),
        &[record("r1", &limited)],
        Mode::Release,
    );
    assert_eq!(
        status_of(&table, "Professional", "x64"),
        Some("conditional")
    );
    let failed = [("native_build", "passed"), ("native_tests", "failed")];
    let table = build(
        &spec(Some(basis(true))),
        &[record("r1", &failed)],
        Mode::Release,
    );
    assert_eq!(
        status_of(&table, "Professional", "x64"),
        Some("unsupported_found")
    );
    let none = [("native_build", "not_run")];
    let table = build(
        &spec(Some(basis(true))),
        &[record("r1", &none)],
        Mode::Release,
    );
    assert_eq!(
        status_of(&table, "Professional", "x64"),
        Some("not_verified")
    );
}

#[test]
fn missing_security_basis_blocks_and_leaves_the_row_out_of_the_embedded_matrix() {
    let table = build(&spec(None), &[], Mode::Development);
    assert_eq!(table.blockers.len(), 4);
    assert!(
        table
            .blockers
            .iter()
            .all(|b| b.reason_key == SUPPORT_BASIS_MISSING)
    );
    assert!(
        table.matrix_json()["combinations"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    let mut partial = basis(true);
    partial.checked_on = Some("not a date".into());
    let table = build(&spec(Some(partial)), &[], Mode::Development);
    assert_eq!(table.blockers.len(), 4);
}

#[test]
fn unconfirmed_basis_is_a_warning_in_development_and_a_blocker_in_release() {
    let development = build(&spec(Some(basis(false))), &[], Mode::Development);
    assert!(development.blockers.is_empty());
    assert_eq!(development.warnings.len(), 4);
    let matrix = development.matrix_json();
    let source = matrix["combinations"][0]["securitySupport"]["source"]
        .as_str()
        .unwrap_or_default();
    assert!(source.ends_with("[unconfirmed]"), "{source}");
    let release = build(&spec(Some(basis(false))), &[], Mode::Release);
    assert_eq!(release.blockers.len(), 4);
    assert!(
        release
            .blockers
            .iter()
            .all(|b| b.reason_key == SUPPORT_BASIS_UNCONFIRMED)
    );
}

#[test]
fn invalid_records_and_keys_are_reported() {
    let checks = [("native_build", "passed")];
    let mut orphan = record("orphan", &checks);
    orphan.release = "11-24H2".into();
    let mut bad_key = record("bad", &checks);
    bad_key.os = "macos".into();
    let mut bad_check = record("badcheck", &[("native_build", "maybe")]);
    bad_check.id = "badcheck".into();
    let records = [
        record("dup", &checks),
        record("dup", &checks),
        orphan,
        bad_key,
        bad_check,
    ];
    let table = build(&spec(Some(basis(true))), &records, Mode::Release);
    let subjects: Vec<_> = table.blockers.iter().map(|b| b.subject.as_str()).collect();
    for expected in ["bad", "badcheck", "dup", "orphan"] {
        assert!(subjects.contains(&expected), "{expected} in {subjects:?}");
    }
    assert!(
        table
            .blockers
            .iter()
            .all(|b| b.reason_key == SUPPORT_EVIDENCE_INVALID)
    );
}

#[test]
fn key_encodings_follow_the_adopted_forms() {
    assert!(keys::validate_key("windows", "11-25H2", "Professional", "x64").is_ok());
    assert!(keys::validate_key("windows", "25H2", "Professional", "x64").is_err());
    assert!(keys::validate_key("macos", "27", "macos", "arm64").is_ok());
    assert!(keys::validate_key("macos", "27.2", "macos", "arm64").is_err());
    assert!(keys::validate_key("macos", "27", "Professional", "arm64").is_err());
    assert!(keys::validate_key("linux", "24.04", "ubuntu", "x64").is_ok());
    assert!(keys::validate_key("linux", "24.04", "ubuntu", "x86_64").is_err());
    assert!(keys::validate_key("plan9", "1", "x", "x64").is_err());
}

#[test]
fn output_is_deterministic_and_row_order_is_stable() -> Result<(), Box<dyn Error>> {
    let checks = [("native_build", "passed")];
    let one = build(
        &spec(Some(basis(true))),
        &[record("r1", &checks)],
        Mode::Release,
    );
    let mut reversed = spec(Some(basis(true)));
    reversed.groups[0].editions.reverse();
    reversed.groups[0].cpus.reverse();
    let two = build(&reversed, &[record("r1", &checks)], Mode::Release);
    assert_eq!(
        serde_json::to_string(&one.matrix_json())?,
        serde_json::to_string(&two.matrix_json())?
    );
    let revision = one.matrix_json()["revision"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert_eq!(
        render_markdown(&one, &revision),
        render_markdown(&two, &revision)
    );
    assert!(revision.starts_with("2026-11-13."));
    let mut changed = record("r1", &checks);
    changed.limits.push("another limit".into());
    let three = build(&spec(Some(basis(true))), &[changed], Mode::Release);
    assert_ne!(Value::String(revision), three.matrix_json()["revision"]);
    Ok(())
}

#[test]
fn matrix_uses_the_core_contract_field_names() {
    let checks = [
        ("native_build", "passed"),
        ("native_tests", "passed"),
        ("installed_start", "limited"),
    ];
    let table = build(
        &spec(Some(basis(true))),
        &[record("r1", &checks)],
        Mode::Release,
    );
    let matrix = table.matrix_json();
    let row = matrix["combinations"].as_array().and_then(|rows| {
        rows.iter()
            .find(|r| r["identity"]["edition"] == "Professional" && r["identity"]["cpu"] == "x64")
    });
    let row = row.cloned().unwrap_or(Value::Null);
    assert_eq!(row["identity"]["osFamily"], "windows");
    assert_eq!(row["status"], "conditional");
    assert_eq!(row["securitySupport"]["kind"], "general");
    assert_eq!(row["verificationRecords"][0], "r1");
    assert!(
        row["verifiedDetails"]
            .as_array()
            .is_some_and(|d| d.len() >= 4)
    );
}
