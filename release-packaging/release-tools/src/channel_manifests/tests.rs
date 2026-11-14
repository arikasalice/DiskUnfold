use super::*;
use crate::release_inventory::{
    DistributionChannel, OsFamily, OsRange, OsVersion, OsVersionScope, Sha256Digest,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn row(
    name: &str,
    format: PackageFormat,
    family: OsFamily,
    signature: SignatureMethod,
) -> Result<DistributionRow, Box<dyn std::error::Error>> {
    let digest = Sha256Digest::parse(&"c".repeat(64))?;
    Ok(DistributionRow {
        file_name: name.into(),
        version: "0.0.1".into(),
        os: OsRange {
            family,
            versions: OsVersionScope::Range {
                minimum: OsVersion::parse("11 25H2")?,
                maximum: None,
            },
        },
        cpu: Cpu::X64,
        format,
        channel: DistributionChannel::Direct,
        signature,
        signature_scope: "scope".into(),
        sha256: digest.clone(),
        executable_file_name: "diskunfold".into(),
        executable_sha256: digest,
        run_instructions: "run".into(),
        sbom_file_name: Some(format!("{name}.cdx.json")),
    })
}

fn current_inventory() -> Result<InventoryDocument, Box<dyn std::error::Error>> {
    Ok(InventoryDocument {
        product_name: "DiskUnfold".into(),
        version: "0.0.1".into(),
        rows: vec![
            row(
                "DiskUnfold-0.0.1-x64-portable.exe",
                PackageFormat::PortableExe,
                OsFamily::Windows,
                SignatureMethod::Unsigned,
            )?,
            row(
                "DiskUnfold-0.0.1-x64.msix",
                PackageFormat::Msix,
                OsFamily::Windows,
                SignatureMethod::Unsigned,
            )?,
            row(
                "DiskUnfold_0.0.1_amd64.deb",
                PackageFormat::Deb,
                OsFamily::Ubuntu,
                SignatureMethod::Unsigned,
            )?,
        ],
        channel_references: Vec::new(),
    })
}

fn official<'a>() -> ChannelContext<'a> {
    ChannelContext {
        repository: "owner/repo",
        tag: Some("v0.0.1"),
        winget_identifier: None,
    }
}

fn parse(files: &[ChannelFile], name: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let file = files
        .iter()
        .find(|file| file.file_name == name)
        .ok_or("missing file")?;
    Ok(serde_json::from_str(&file.contents)?)
}

#[test]
fn direct_lists_every_package_with_its_release_page_address() -> TestResult {
    let files = build_channel_manifests(&current_inventory()?, &official())?;
    let direct = parse(&files, "channel-direct.json")?;
    assert_eq!(direct["status"], "pending_publication");
    assert_eq!(direct["update_method"], "replace_file");
    assert_eq!(direct["entries"].as_array().ok_or("entries")?.len(), 3);
    assert_eq!(
        direct["entries"][2]["url"],
        "https://github.com/owner/repo/releases/download/v0.0.1/DiskUnfold_0.0.1_amd64.deb"
    );
    assert_eq!(
        direct["entries"][2]["sbom_file_name"],
        "DiskUnfold_0.0.1_amd64.deb.cdx.json"
    );
    Ok(())
}

#[test]
fn winget_and_store_are_marked_unavailable_with_the_design_key() -> TestResult {
    let files = build_channel_manifests(&current_inventory()?, &official())?;
    for name in ["channel-winget.json", "channel-store.json"] {
        let document = parse(&files, name)?;
        assert_eq!(document["status"], "unavailable", "{name}");
        assert_eq!(
            document["reason_key"], "release.format_unavailable",
            "{name}"
        );
        assert_eq!(document["entries"], json!([]), "{name}");
    }
    Ok(())
}

#[test]
fn a_developer_build_is_never_described_as_published() -> TestResult {
    let context = ChannelContext {
        repository: "owner/repo",
        tag: None,
        winget_identifier: None,
    };
    let files = build_channel_manifests(&current_inventory()?, &context)?;
    let direct = parse(&files, "channel-direct.json")?;
    assert_eq!(direct["status"], "developer_build");
    assert_eq!(direct["build_kind"], "developer");
    assert_eq!(direct["entries"][0]["url"], Value::Null);
    assert_eq!(direct["release_page"], Value::Null);
    Ok(())
}

#[test]
fn winget_needs_an_msi_and_an_identifier_and_an_official_build() -> TestResult {
    let mut inventory = current_inventory()?;
    inventory.rows.push(row(
        "DiskUnfold-0.0.1-x64.msi",
        PackageFormat::Msi,
        OsFamily::Windows,
        SignatureMethod::SignPath,
    )?);
    let without_identifier = parse(
        &build_channel_manifests(&inventory, &official())?,
        "channel-winget.json",
    )?;
    assert_eq!(without_identifier["status"], "unavailable");
    assert_eq!(without_identifier["reason_key"], Value::Null);

    let context = ChannelContext {
        winget_identifier: Some("Publisher.DiskUnfold"),
        ..official()
    };
    let available = parse(
        &build_channel_manifests(&inventory, &context)?,
        "channel-winget.json",
    )?;
    assert_eq!(available["status"], "available");
    let entry = &available["entries"][0];
    assert_eq!(entry["package_identifier"], "Publisher.DiskUnfold");
    assert_eq!(entry["installer_type"], "msi");
    assert_eq!(entry["update_method"], "channel_command");
    assert_eq!(entry["installer_sha256"], "c".repeat(64));

    let developer = ChannelContext {
        tag: None,
        ..context
    };
    let blocked = parse(
        &build_channel_manifests(&inventory, &developer)?,
        "channel-winget.json",
    )?;
    assert_eq!(blocked["status"], "unavailable");
    Ok(())
}

#[test]
fn store_is_available_only_for_a_store_signed_msix() -> TestResult {
    let mut inventory = current_inventory()?;
    inventory.rows.push(row(
        "DiskUnfold-0.0.1-arm64.msix",
        PackageFormat::Msix,
        OsFamily::Windows,
        SignatureMethod::Store,
    )?);
    let store = parse(
        &build_channel_manifests(&inventory, &official())?,
        "channel-store.json",
    )?;
    assert_eq!(store["status"], "available");
    assert_eq!(store["entries"].as_array().ok_or("entries")?.len(), 1);
    assert_eq!(
        store["entries"][0]["file_name"],
        "DiskUnfold-0.0.1-arm64.msix"
    );
    assert_eq!(store["update_method"], "automatic");
    Ok(())
}

#[test]
fn output_is_deterministic() -> TestResult {
    let inventory = current_inventory()?;
    assert_eq!(
        build_channel_manifests(&inventory, &official())?,
        build_channel_manifests(&inventory, &official())?
    );
    Ok(())
}
