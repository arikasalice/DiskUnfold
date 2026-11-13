use super::*;
use crate::{
    notices::{DenyPolicy, Ecosystem, LicenseText, NoticePackage, check_policy, render_notices},
    support_table::{BasisSpec, GroupSpec, ReleaseSpec, SupportTableSpec, build},
};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn table(confirmed: bool) -> SupportTable {
    build(
        &SupportTableSpec {
            as_of: "2026-11-13".into(),
            groups: vec![GroupSpec {
                os: "macos".into(),
                releases: vec![ReleaseSpec {
                    release: "27".into(),
                    security_basis: Some(BasisSpec {
                        kind: Some("general".into()),
                        source: Some("https://example.invalid".into()),
                        checked_on: Some("2026-11-13".into()),
                        confirmed,
                        vendor_end_date: None,
                    }),
                }],
                editions: vec!["macos".into()],
                cpus: vec!["arm64".into()],
            }],
        },
        &[],
        if confirmed {
            Mode::Release
        } else {
            Mode::Development
        },
    )
}

fn report(license: &str) -> NoticeReport {
    let packages = [NoticePackage {
        ecosystem: Ecosystem::Cargo,
        name: "serde".into(),
        version: "1.0.0".into(),
        license: Some(license.into()),
        texts: vec![LicenseText {
            file: "LICENSE".into(),
            text: "MIT text".into(),
        }],
        authors: Vec::new(),
    }];
    let policy = DenyPolicy {
        allowed: ["MIT".to_owned()].into(),
        banned_crates: Default::default(),
    };
    NoticeReport {
        text: render_notices(&packages),
        package_count: 1,
        violations: check_policy(&packages, &policy),
    }
}

// launcher/build_info.rs reads exactly these pointers from the resources file.
#[test]
fn passing_release_resources_carry_every_field_the_build_script_reads() {
    let resources = compose(Some(COMMIT), &report("MIT"), &table(true));
    assert!(resources.blockers.is_empty());
    let json = &resources.json;
    assert_eq!(json.pointer("/sourceCommit"), Some(&Value::from(COMMIT)));
    assert_eq!(
        json.pointer("/releaseGate/passed"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        json.pointer("/releaseGate/mode"),
        Some(&Value::from("release"))
    );
    assert_eq!(
        json.pointer("/thirdPartyNotices"),
        Some(&Value::from(resources.notices.clone()))
    );
    assert_eq!(
        json.pointer("/supportMatrix"),
        Some(&resources.support_matrix)
    );
    assert!(
        resources.support_matrix["revision"]
            .as_str()
            .is_some_and(|r| r.starts_with("2026-11-13."))
    );
}

#[test]
fn a_license_violation_or_missing_commit_fails_the_gate() {
    let denied = compose(Some(COMMIT), &report("GPL-3.0-only"), &table(true));
    assert_eq!(denied.blockers.len(), 1);
    assert_eq!(
        denied.json.pointer("/releaseGate/passed"),
        Some(&Value::Bool(false))
    );
    let no_commit = compose(None, &report("MIT"), &table(true));
    assert!(
        no_commit
            .blockers
            .iter()
            .any(|b| b.reason_key == SOURCE_COMMIT_MISSING)
    );
    assert_eq!(
        no_commit.json.pointer("/releaseGate/passed"),
        Some(&Value::Bool(false))
    );
}

#[test]
fn development_resources_are_marked_as_development_mode() {
    let resources = compose(Some(COMMIT), &report("MIT"), &table(false));
    assert_eq!(
        resources.json.pointer("/releaseGate/mode"),
        Some(&Value::from("development"))
    );
    assert!(
        resources.json["releaseGate"]["warnings"]
            .as_array()
            .is_some_and(|w| !w.is_empty())
    );
}
