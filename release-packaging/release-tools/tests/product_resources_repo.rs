//! Runs the generators against this repository.

use std::{error::Error, path::PathBuf};

use du_release_tools::{product_resources::generate, support_table::Mode};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn repository_resources_are_deterministic_and_pass_the_license_policy() -> Result<(), Box<dyn Error>>
{
    let commit = Some("0123456789abcdef0123456789abcdef01234567".to_owned());
    let first = generate(&repo(), commit.clone(), Mode::Development)?;
    let second = generate(&repo(), commit, Mode::Development)?;
    assert_eq!(first.notices, second.notices);
    assert_eq!(first.json, second.json);
    assert_eq!(first.support_markdown, second.support_markdown);
    assert!(
        first.notices.contains("cargo serde "),
        "launcher dependencies are listed"
    );
    assert!(
        first.notices.contains("npm react "),
        "gui-web production packages are listed"
    );
    assert!(
        !first.notices.contains("cargo du-"),
        "first-party crates are covered by LICENSE"
    );
    assert!(
        first
            .blockers
            .iter()
            .all(|b| !b.reason_key.starts_with("release.notices_")),
        "{:?}",
        first.blockers
    );
    Ok(())
}

#[test]
fn every_row_has_a_confirmed_basis_so_both_modes_pass_the_support_gate()
-> Result<(), Box<dyn Error>> {
    let commit = Some("0123456789abcdef0123456789abcdef01234567".to_owned());
    let development = generate(&repo(), commit.clone(), Mode::Development)?;
    assert!(
        development.blockers.is_empty(),
        "{:?}",
        development.blockers
    );
    assert_eq!(development.json["releaseGate"]["passed"], true);

    // The user confirmed every security-support basis on 2026-11-13, so release mode
    // reports no support-basis blocker.
    let release = generate(&repo(), commit, Mode::Release)?;
    assert!(
        release
            .blockers
            .iter()
            .all(|b| !b.reason_key.starts_with("release.support_basis_")),
        "{:?}",
        release.blockers
    );
    assert_eq!(release.json["releaseGate"]["passed"], true);

    let rows = release.support_matrix["combinations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        rows.len(),
        18,
        "every row has a basis, so every row is embedded"
    );
    let ubuntu = rows
        .iter()
        .find(|r| r["identity"]["release"] == "24.04" && r["identity"]["cpu"] == "x64")
        .cloned()
        .unwrap_or_default();
    assert_eq!(ubuntu["status"], "conditional");
    assert_eq!(
        ubuntu["verificationRecords"][0],
        "ubuntu-24.04-ubuntu-x64-container-2026-11-13"
    );
    assert_eq!(
        ubuntu["securitySupport"]["source"],
        "https://ubuntu.com/about/release-cycle"
    );
    Ok(())
}
