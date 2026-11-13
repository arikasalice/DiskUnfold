use super::*;
use crate::notices::{LicenseText, render_notices};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn package(
    ecosystem: Ecosystem,
    name: &str,
    version: &str,
    license: Option<&str>,
) -> NoticePackage {
    NoticePackage {
        ecosystem,
        name: name.into(),
        version: version.into(),
        license: license.map(str::to_owned),
        texts: vec![LicenseText {
            file: "LICENSE".into(),
            text: "licence text".into(),
        }],
        authors: Vec::new(),
    }
}

fn packages() -> Vec<NoticePackage> {
    vec![
        package(
            Ecosystem::Cargo,
            "serde",
            "1.0.1",
            Some("MIT OR Apache-2.0"),
        ),
        package(Ecosystem::Cargo, "zstd", "0.13.0", Some("BSD-3-Clause")),
        package(Ecosystem::Npm, "@scope/widget", "2.0.0", Some("MIT")),
        package(Ecosystem::Npm, "odd", "1.0.0", Some("see file")),
    ]
}

const SUBJECT: SbomSubject<'static> = SbomSubject {
    product_name: "DiskUnfold",
    version: "0.1.0",
    file_name: "DiskUnfold-0.1.0-linux-x86_64.tar.gz",
    sha256: "aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11",
};

fn render(packages: &[NoticePackage]) -> Result<String, serde_json::Error> {
    render_cyclonedx(
        &SUBJECT,
        packages,
        &[BuildTool {
            name: "rustc",
            version: "1.99.1",
        }],
    )
}

#[test]
fn renders_a_cyclonedx_document_for_the_package() -> TestResult {
    let text = render(&packages())?;
    let value: Value = serde_json::from_str(&text)?;
    assert_eq!(value["bomFormat"], "CycloneDX");
    assert_eq!(value["specVersion"], "1.5");
    assert_eq!(
        value["metadata"]["component"]["hashes"][0]["content"],
        SUBJECT.sha256
    );
    assert_eq!(sbom_subject_sha256(&text)?, SUBJECT.sha256);
    let first = &value["components"][0];
    assert_eq!(first["purl"], "pkg:cargo/serde@1.0.1");
    assert_eq!(first["licenses"][0]["expression"], "MIT OR Apache-2.0");
    assert!(
        value["serialNumber"]
            .as_str()
            .is_some_and(|s| s.starts_with("urn:uuid:"))
    );
    Ok(())
}

#[test]
fn encodes_scoped_npm_names_and_keeps_unparsable_licenses_as_names() -> TestResult {
    let value: Value = serde_json::from_str(&render(&packages())?)?;
    let components = value["components"].as_array().ok_or("components")?;
    let scoped = components
        .iter()
        .find(|c| c["name"] == "@scope/widget")
        .ok_or("scoped")?;
    assert_eq!(scoped["purl"], "pkg:npm/%40scope/widget@2.0.0");
    let odd = components
        .iter()
        .find(|c| c["name"] == "odd")
        .ok_or("odd")?;
    assert_eq!(odd["licenses"][0]["license"]["name"], "see file");
    Ok(())
}

#[test]
fn build_tools_are_excluded_from_the_compared_components() -> TestResult {
    let text = render(&packages())?;
    let value: Value = serde_json::from_str(&text)?;
    let tool = value["components"]
        .as_array()
        .ok_or("components")?
        .last()
        .ok_or("tool")?;
    assert_eq!(tool["name"], "rustc");
    assert_eq!(tool["scope"], "excluded");
    assert_eq!(sbom_components(&text)?.len(), 4);
    Ok(())
}

#[test]
fn output_is_deterministic_and_ignores_input_order() -> TestResult {
    let mut reversed = packages();
    reversed.reverse();
    reversed.push(package(
        Ecosystem::Cargo,
        "serde",
        "1.0.1",
        Some("MIT OR Apache-2.0"),
    ));
    assert_eq!(render(&packages())?, render(&reversed)?);
    Ok(())
}

#[test]
fn an_sbom_made_from_the_same_packages_matches_the_notices() -> TestResult {
    let notices = render_notices(&packages());
    check_sbom_matches_notices(&render(&packages())?, &notices)?;
    Ok(())
}

#[test]
fn a_package_on_one_side_only_fails_with_the_design_key() -> TestResult {
    let notices = render_notices(&packages());
    let mut fewer = packages();
    fewer.pop();
    let failure = check_sbom_matches_notices(&render(&fewer)?, &notices)
        .err()
        .ok_or("a missing SBOM entry must fail")?;
    assert_eq!(failure.reason_key, "release.notice_mismatch");
    assert!(
        failure
            .detail
            .contains("only in THIRD-PARTY-NOTICES: npm odd 1.0.0"),
        "{failure}"
    );

    let mut more = packages();
    more.push(package(Ecosystem::Cargo, "extra", "0.1.0", Some("MIT")));
    let failure = check_sbom_matches_notices(&render(&more)?, &notices)
        .err()
        .ok_or("an extra SBOM entry must fail")?;
    assert!(
        failure
            .detail
            .contains("only in the SBOM: cargo extra 0.1.0"),
        "{failure}"
    );
    Ok(())
}

#[test]
fn a_version_difference_is_a_mismatch() -> TestResult {
    let notices = render_notices(&packages());
    let mut changed = packages();
    changed[0].version = "1.0.2".into();
    assert!(check_sbom_matches_notices(&render(&changed)?, &notices).is_err());
    Ok(())
}

#[test]
fn unreadable_documents_fail_closed() {
    assert!(sbom_components("not json").is_err());
    assert!(sbom_components("{\"bomFormat\":\"SPDX\"}").is_err());
    assert!(sbom_components("{\"bomFormat\":\"CycloneDX\"}").is_err());
    assert!(sbom_subject_sha256("{\"bomFormat\":\"CycloneDX\"}").is_err());
    assert!(notice_identities("DiskUnfold THIRD-PARTY-NOTICES\n\nPackages: 0\n").is_err());
}

#[test]
fn notice_identities_stop_at_the_first_license_text() -> TestResult {
    let text = "cargo a 1.0.0: MIT\nnpm b 2.0.0: ISC\n\n========\ncargo c 3.0.0: MIT\n";
    let ids = notice_identities(text)?;
    assert_eq!(ids.len(), 2);
    assert!(!ids.contains(&("cargo".into(), "c".into(), "3.0.0".into())));
    Ok(())
}

#[test]
fn sbom_file_names_follow_the_package_name() {
    assert_eq!(
        sbom_file_name("DiskUnfold-0.1.0-x64.msi"),
        "DiskUnfold-0.1.0-x64.msi.cdx.json"
    );
}
