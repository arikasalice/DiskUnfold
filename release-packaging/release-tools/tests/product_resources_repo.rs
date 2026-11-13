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
fn ubuntu_rows_without_a_confirmed_basis_stop_a_release() -> Result<(), Box<dyn Error>> {
    let commit = Some("0123456789abcdef0123456789abcdef01234567".to_owned());
    let release = generate(&repo(), commit, Mode::Release)?;
    assert_eq!(release.json["releaseGate"]["passed"], false);
    let blockers = &release.blockers;
    let missing = blockers
        .iter()
        .filter(|b| b.reason_key == "release.support_basis_missing");
    assert_eq!(missing.count(), 4, "{blockers:?}");
    let matrix = release.support_matrix["combinations"]
        .as_array()
        .map_or(0, Vec::len);
    assert_eq!(
        matrix, 14,
        "rows without a basis stay out of the embedded matrix"
    );
    Ok(())
}
