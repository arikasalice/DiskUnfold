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
    version: "0.0.1",
    file_name: "DiskUnfold-0.0.1-linux-x86_64.tar.gz",
    sha256: "aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11",
    rust_targets: &["x86_64-unknown-linux-gnu"],
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

fn ids(packages: &[NoticePackage]) -> BTreeSet<ComponentId> {
    packages
        .iter()
        .map(|p| {
            (
                p.ecosystem.label().to_owned(),
                p.name.clone(),
                p.version.clone(),
            )
        })
        .collect()
}

#[test]
fn an_sbom_made_from_the_same_packages_is_covered_by_the_notices() -> TestResult {
    let notices = render_notices(&packages());
    check_sbom_coverage(&render(&packages())?, &notices, None)?;
    check_sbom_coverage(&render(&packages())?, &notices, Some(&ids(&packages())))?;
    Ok(())
}

#[test]
fn notices_may_list_more_than_one_target_sbom() -> TestResult {
    // The notices cover every target, so a Windows SBOM is a subset of them.
    let notices = render_notices(&packages());
    let mut fewer = packages();
    fewer.pop();
    check_sbom_coverage(&render(&fewer)?, &notices, Some(&ids(&fewer)))?;
    Ok(())
}

#[test]
fn an_sbom_entry_the_notices_lack_fails_with_the_design_key() -> TestResult {
    let notices = render_notices(&packages());
    let mut more = packages();
    more.push(package(Ecosystem::Cargo, "extra", "0.1.0", Some("MIT")));
    let failure = check_sbom_coverage(&render(&more)?, &notices, None)
        .err()
        .ok_or("an SBOM entry without a notice must fail")?;
    assert_eq!(failure.reason_key, "release.notice_mismatch");
    assert!(failure.detail.contains("cargo extra 0.1.0"), "{failure}");
    Ok(())
}

#[test]
fn the_independent_expected_set_catches_a_package_both_documents_drop() -> TestResult {
    // Both documents lack "odd"; only the independently derived set knows it is shipped.
    let mut fewer = packages();
    fewer.pop();
    let notices = render_notices(&fewer);
    let failure = check_sbom_coverage(&render(&fewer)?, &notices, Some(&ids(&packages())))
        .err()
        .ok_or("a package missing everywhere must fail")?;
    assert!(
        failure
            .detail
            .contains("missing from the SBOM: npm odd 1.0.0"),
        "{failure}"
    );
    assert!(
        failure
            .detail
            .contains("missing from THIRD-PARTY-NOTICES: npm odd 1.0.0"),
        "{failure}"
    );
    Ok(())
}

#[test]
fn an_sbom_with_an_unexpected_package_fails() -> TestResult {
    let notices = render_notices(&packages());
    let mut expected = packages();
    expected.remove(0);
    let failure = check_sbom_coverage(&render(&packages())?, &notices, Some(&ids(&expected)))
        .err()
        .ok_or("an unexpected package must fail")?;
    assert!(
        failure
            .detail
            .contains("not expected for this target: cargo serde 1.0.1"),
        "{failure}"
    );
    Ok(())
}

#[test]
fn a_version_difference_is_a_mismatch() -> TestResult {
    let notices = render_notices(&packages());
    let mut changed = packages();
    changed[0].version = "1.0.2".into();
    assert!(check_sbom_coverage(&render(&changed)?, &notices, None).is_err());
    Ok(())
}

#[test]
fn purls_percent_encode_build_metadata() -> TestResult {
    let value: Value = serde_json::from_str(&render(&[package(
        Ecosystem::Cargo,
        "toml_edit",
        "0.25.19+spec-1.1.0",
        Some("MIT"),
    )])?)?;
    assert_eq!(
        value["components"][0]["purl"],
        "pkg:cargo/toml_edit@0.25.19%2Bspec-1.1.0"
    );
    // The component version keeps the plain text, so identities still compare.
    assert_eq!(value["components"][0]["version"], "0.25.19+spec-1.1.0");
    Ok(())
}

#[test]
fn each_package_is_built_for_its_own_rust_targets() -> TestResult {
    assert_eq!(
        rust_targets(OsFamily::Windows, Cpu::X64),
        Some(&["x86_64-pc-windows-msvc"][..])
    );
    assert_eq!(
        rust_targets(OsFamily::Macos, Cpu::Universal).map(<[&str]>::len),
        Some(2)
    );
    assert_eq!(rust_targets(OsFamily::Ubuntu, Cpu::Universal), None);
    let value: Value = serde_json::from_str(&render(&packages())?)?;
    assert_eq!(
        value["metadata"]["component"]["properties"][1]["value"],
        "x86_64-unknown-linux-gnu"
    );
    Ok(())
}

#[test]
fn the_serial_number_depends_on_targets_and_build_tools() -> TestResult {
    let other = SbomSubject {
        rust_targets: &["aarch64-unknown-linux-gnu"],
        ..SUBJECT
    };
    let tool = [BuildTool {
        name: "rustc",
        version: "1.99.1",
    }];
    let first: Value = serde_json::from_str(&render_cyclonedx(&SUBJECT, &packages(), &tool)?)?;
    let second: Value = serde_json::from_str(&render_cyclonedx(&other, &packages(), &tool)?)?;
    assert_ne!(first["serialNumber"], second["serialNumber"]);
    let third: Value = serde_json::from_str(&render_cyclonedx(&SUBJECT, &packages(), &[])?)?;
    assert_ne!(first["serialNumber"], third["serialNumber"]);
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
        sbom_file_name("DiskUnfold-0.0.1-x64.msi"),
        "DiskUnfold-0.0.1-x64.msi.cdx.json"
    );
}

#[test]
fn the_sbom_holds_no_microsoft_download_link() -> TestResult {
    let text = render(&packages())?;
    assert_eq!(
        crate::release_inventory::microsoft_download_reference(&text),
        None
    );
    assert!(!text.contains("WebView2 Evergreen"));
    Ok(())
}
