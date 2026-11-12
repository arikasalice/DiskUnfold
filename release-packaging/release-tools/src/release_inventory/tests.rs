use super::*;
use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::Path};
use tempfile::TempDir;

fn sample_manifest(root: &Path) -> Result<ReleaseManifest, Box<dyn Error>> {
    fs::write(root.join("DiskUnfold-0.1.0-x64.msi"), b"package")?;
    fs::write(root.join("DiskUnfold.exe"), b"executable")?;
    let row = DistributionRow {
        file_name: "DiskUnfold-0.1.0-x64.msi".into(),
        version: "0.1.0".into(),
        os: OsRange {
            family: OsFamily::Windows,
            minimum: OsVersion::parse("11 25H2")?,
            maximum: None,
        },
        cpu: Cpu::X64,
        format: PackageFormat::Msi,
        channel: DistributionChannel::Direct,
        signature: SignatureMethod::SignPath,
        signature_scope: "Authenticode identity and signed package integrity".into(),
        sha256: Sha256Digest::parse(&sha256(b"package"))?,
        executable_file_name: "DiskUnfold.exe".into(),
        executable_sha256: Sha256Digest::parse(&sha256(b"executable"))?,
        run_instructions: "Install the MSI, then launch DiskUnfold.".into(),
    };
    Ok(ReleaseManifest {
        product_name: "DiskUnfold".into(),
        version: "0.1.0".into(),
        rows: vec![row],
        channel_references: vec![ChannelReference {
            channel: UpdateChannel::Winget,
            version: "0.1.0".into(),
            cpu: Cpu::X64,
            artifact_file_name: "DiskUnfold-0.1.0-x64.msi".into(),
            update_method: UpdateMethod::ChannelCommand,
        }],
        staged_files: vec![
            StagedFile {
                artifact_file_name: "DiskUnfold-0.1.0-x64.msi".into(),
                kind: StagedFileKind::Package,
                relative_path: "DiskUnfold-0.1.0-x64.msi".into(),
            },
            StagedFile {
                artifact_file_name: "DiskUnfold-0.1.0-x64.msi".into(),
                kind: StagedFileKind::Executable,
                relative_path: "DiskUnfold.exe".into(),
            },
        ],
    })
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[test]
fn sha256_uses_the_standard_sha256_encoding() {
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn validates_channel_links_and_reverifies_package_and_executable() -> Result<(), Box<dyn Error>> {
    let staging = TempDir::new()?;
    let manifest = sample_manifest(staging.path())?;
    let (inventory, verified) = prepare_inventory(manifest, staging.path())?;

    assert_eq!(inventory.rows[0].file_name, "DiskUnfold-0.1.0-x64.msi");
    assert_eq!(
        inventory.channel_references[0].channel,
        UpdateChannel::Winget
    );
    assert_eq!(verified.len(), 2);
    assert_eq!(verified[0].kind, StagedFileKind::Package);
    assert_eq!(verified[1].kind, StagedFileKind::Executable);
    Ok(())
}

#[test]
fn sorts_rows_by_format_then_os_then_cpu_deterministically() -> Result<(), Box<dyn Error>> {
    let staging = TempDir::new()?;
    let mut manifest = sample_manifest(staging.path())?;
    let mut mac = manifest.rows[0].clone();
    mac.file_name = "DiskUnfold-0.1.0-universal.dmg".into();
    mac.os = OsRange {
        family: OsFamily::Macos,
        minimum: OsVersion::parse("15")?,
        maximum: Some(OsVersion::parse("27")?),
    };
    mac.cpu = Cpu::Universal;
    mac.format = PackageFormat::Dmg;
    manifest.rows.push(mac.clone());
    let mut deb = mac.clone();
    deb.file_name = "DiskUnfold-0.1.0-arm64.deb".into();
    deb.os = OsRange {
        family: OsFamily::Ubuntu,
        minimum: OsVersion::parse("24.04")?,
        maximum: Some(OsVersion::parse("26.04")?),
    };
    deb.cpu = Cpu::Arm64;
    deb.format = PackageFormat::Deb;
    manifest.rows.push(deb);

    let mut ubuntu_x64 = mac.clone();
    ubuntu_x64.file_name = "DiskUnfold-0.1.0-x86_64.tar.gz".into();
    ubuntu_x64.os = OsRange {
        family: OsFamily::Ubuntu,
        minimum: OsVersion::parse("24.04")?,
        maximum: Some(OsVersion::parse("26.04")?),
    };
    ubuntu_x64.cpu = Cpu::X64;
    ubuntu_x64.format = PackageFormat::TarGz;
    manifest.rows.push(ubuntu_x64.clone());

    let mut ubuntu_arm64 = ubuntu_x64.clone();
    ubuntu_arm64.file_name = "DiskUnfold-0.1.0-aarch64.tar.gz".into();
    ubuntu_arm64.cpu = Cpu::Arm64;
    manifest.rows.push(ubuntu_arm64.clone());

    let mut windows_arm64 = ubuntu_arm64;
    windows_arm64.file_name = "DiskUnfold-0.1.0-arm64.tar.gz".into();
    windows_arm64.os = OsRange {
        family: OsFamily::Windows,
        minimum: OsVersion::parse("11 25H2")?,
        maximum: Some(OsVersion::parse("11 26H2")?),
    };
    manifest.rows.push(windows_arm64);
    manifest.staged_files.clear();
    for row in &manifest.rows {
        manifest.staged_files.push(StagedFile {
            artifact_file_name: row.file_name.clone(),
            kind: StagedFileKind::Package,
            relative_path: row.file_name.clone().into(),
        });
        manifest.staged_files.push(StagedFile {
            artifact_file_name: row.file_name.clone(),
            kind: StagedFileKind::Executable,
            relative_path: row.executable_file_name.clone().into(),
        });
    }
    validate_and_sort(&mut manifest)?;
    let order: Vec<_> = manifest
        .rows
        .iter()
        .map(|row| row.format.sort_key())
        .collect();
    assert_eq!(order, ["deb", "dmg", "msi", "tar.gz", "tar.gz", "tar.gz"]);
    assert_eq!(
        manifest.rows[3..]
            .iter()
            .map(|row| (row.os.family, row.cpu))
            .collect::<Vec<_>>(),
        [
            (OsFamily::Ubuntu, Cpu::Arm64),
            (OsFamily::Ubuntu, Cpu::X64),
            (OsFamily::Windows, Cpu::Arm64),
        ]
    );
    let output = ReleaseInventory {
        product_name: manifest.product_name.clone(),
        version: manifest.version.clone(),
        rows: manifest.rows.clone(),
        channel_references: manifest.channel_references.clone(),
    };
    let json = render_json(&output)?;
    assert_eq!(json, render_json(&output)?);
    Ok(())
}

#[test]
fn accepts_published_cpu_filename_aliases() -> Result<(), Box<dyn Error>> {
    let staging = TempDir::new()?;
    let mut manifest = sample_manifest(staging.path())?;
    manifest.rows[0].file_name = "DiskUnfold-0.1.0-x86_64.msi".into();
    manifest.channel_references[0].artifact_file_name = manifest.rows[0].file_name.clone();
    for staged in &mut manifest.staged_files {
        staged.artifact_file_name = manifest.rows[0].file_name.clone();
        if staged.kind == StagedFileKind::Package {
            staged.relative_path = manifest.rows[0].file_name.clone().into();
        }
    }
    validate_and_sort(&mut manifest)?;

    manifest.rows[0].file_name = "DiskUnfold-0.1.0-x640.msi".into();
    manifest.channel_references[0].artifact_file_name = manifest.rows[0].file_name.clone();
    for staged in &mut manifest.staged_files {
        staged.artifact_file_name = manifest.rows[0].file_name.clone();
        if staged.kind == StagedFileKind::Package {
            staged.relative_path = manifest.rows[0].file_name.clone().into();
        }
    }
    assert!(matches!(
        validate_and_sort(&mut manifest),
        Err(InventoryError::InvalidField {
            field: "file_name",
            ..
        })
    ));
    Ok(())
}

#[test]
fn rejects_bad_digest_and_missing_channel_target() -> Result<(), Box<dyn Error>> {
    assert!(serde_json::from_str::<Sha256Digest>("\"abc\"").is_err());

    let staging = TempDir::new()?;
    let mut manifest = sample_manifest(staging.path())?;
    manifest.channel_references[0].artifact_file_name = "missing-x64.msi".into();
    assert!(matches!(
        validate_and_sort(&mut manifest),
        Err(InventoryError::InvalidChannelReference(_))
    ));
    Ok(())
}

#[test]
fn reports_staged_digest_mismatch_and_rejects_parent_traversal() -> Result<(), Box<dyn Error>> {
    let staging = TempDir::new()?;
    let mut manifest = sample_manifest(staging.path())?;
    manifest.rows[0].sha256 = Sha256Digest::parse(&sha256(b"different"))?;
    assert!(matches!(
        prepare_inventory(manifest, staging.path()),
        Err(InventoryError::DigestMismatch {
            kind: StagedFileKind::Package,
            ..
        })
    ));

    let mut manifest = sample_manifest(staging.path())?;
    manifest.staged_files[0].relative_path = "../outside.msi".into();
    assert!(matches!(
        validate_and_sort(&mut manifest),
        Err(InventoryError::InvalidStagedFile(_))
    ));

    let mut manifest = sample_manifest(staging.path())?;
    manifest.rows[0].os.maximum = Some(OsVersion::parse("11 24H2")?);
    assert!(matches!(
        validate_and_sort(&mut manifest),
        Err(InventoryError::InvalidField { field: "os", .. })
    ));
    Ok(())
}

#[test]
fn renders_table_without_local_staging_paths() -> Result<(), Box<dyn Error>> {
    let staging = TempDir::new()?;
    let (inventory, _) = prepare_inventory(sample_manifest(staging.path())?, staging.path())?;
    let markdown = render_markdown(&inventory);
    assert!(markdown.contains("DiskUnfold-0.1.0-x64.msi"));
    assert!(markdown.contains("channel command"));
    assert!(markdown.contains("not verified or created by this tool"));
    assert!(markdown.contains("not a release gate"));
    assert!(!markdown.contains(staging.path().to_string_lossy().as_ref()));
    Ok(())
}
