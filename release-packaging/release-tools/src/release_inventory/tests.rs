use super::*;
use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::Path};
use tempfile::TempDir;

use super::fixtures::{sample_manifest, sha256};

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
        versions: OsVersionScope::Range {
            minimum: OsVersion::parse("15")?,
            maximum: Some(OsVersion::parse("27")?),
        },
    };
    mac.cpu = Cpu::Universal;
    mac.format = PackageFormat::Dmg;
    mac.bundled_components.clear();
    manifest.rows.push(mac.clone());
    let mut deb = mac.clone();
    deb.file_name = "DiskUnfold-0.1.0-arm64.deb".into();
    deb.os = OsRange {
        family: OsFamily::Ubuntu,
        versions: OsVersionScope::Exact {
            supported_versions: vec![OsVersion::parse("24.04")?, OsVersion::parse("26.04")?],
        },
    };
    deb.cpu = Cpu::Arm64;
    deb.format = PackageFormat::Deb;
    deb.bundled_components.clear();
    manifest.rows.push(deb);

    let mut ubuntu_x64 = mac.clone();
    ubuntu_x64.file_name = "DiskUnfold-0.1.0-x86_64.tar.gz".into();
    ubuntu_x64.os = OsRange {
        family: OsFamily::Ubuntu,
        versions: OsVersionScope::Exact {
            supported_versions: vec![OsVersion::parse("24.04")?, OsVersion::parse("26.04")?],
        },
    };
    ubuntu_x64.cpu = Cpu::X64;
    ubuntu_x64.format = PackageFormat::TarGz;
    ubuntu_x64.bundled_components.clear();
    manifest.rows.push(ubuntu_x64.clone());

    let mut ubuntu_arm64 = ubuntu_x64.clone();
    ubuntu_arm64.file_name = "DiskUnfold-0.1.0-aarch64.tar.gz".into();
    ubuntu_arm64.cpu = Cpu::Arm64;
    manifest.rows.push(ubuntu_arm64.clone());

    let mut windows_arm64 = ubuntu_arm64;
    windows_arm64.file_name = "DiskUnfold-0.1.0-arm64.tar.gz".into();
    windows_arm64.os = OsRange {
        family: OsFamily::Windows,
        versions: OsVersionScope::Range {
            minimum: OsVersion::parse("11 25H2")?,
            maximum: Some(OsVersion::parse("11 26H2")?),
        },
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
        fuzz_gate: None,
    };
    let json = render_json(&output)?;
    assert_eq!(json, render_json(&output)?);
    Ok(())
}

#[test]
fn preserves_legacy_os_range_manifest_shape() -> Result<(), Box<dyn Error>> {
    let legacy = serde_json::json!({
        "family": "windows",
        "minimum": "11 25H2",
        "maximum": null
    });
    let parsed: OsRange = serde_json::from_value(legacy.clone())?;
    assert_eq!(serde_json::to_value(parsed)?, legacy);
    Ok(())
}

#[test]
fn renders_exact_os_versions_without_implying_intermediate_support() -> Result<(), Box<dyn Error>> {
    let staging = TempDir::new()?;
    let mut manifest = sample_manifest(staging.path())?;
    manifest.channel_references.clear();
    manifest.rows[0].file_name = "DiskUnfold-0.1.0-x64.tar.gz".into();
    manifest.rows[0].os = OsRange {
        family: OsFamily::Ubuntu,
        versions: OsVersionScope::Exact {
            supported_versions: vec![OsVersion::parse("26.04")?, OsVersion::parse("24.04")?],
        },
    };
    manifest.rows[0].format = PackageFormat::TarGz;
    manifest.rows[0].bundled_components.clear();
    manifest.rows[0].signature = SignatureMethod::Unsigned;
    for staged in &mut manifest.staged_files {
        staged.artifact_file_name = manifest.rows[0].file_name.clone();
        if staged.kind == StagedFileKind::Package {
            staged.relative_path = manifest.rows[0].file_name.clone().into();
        }
    }

    validate_and_sort(&mut manifest)?;
    let inventory = ReleaseInventory {
        product_name: manifest.product_name,
        version: manifest.version,
        rows: manifest.rows,
        channel_references: manifest.channel_references,
        fuzz_gate: None,
    };
    let json = render_json(&inventory)?;
    let output: serde_json::Value = serde_json::from_str(&json)?;
    let rows = output["rows"]
        .as_array()
        .ok_or("inventory rows are missing")?;
    assert_eq!(
        rows.len(),
        1,
        "one Linux package covers both exact versions"
    );
    assert_eq!(rows[0]["os"]["family"], "ubuntu");
    assert_eq!(
        rows[0]["os"]["supported_versions"],
        serde_json::json!(["24.04", "26.04"])
    );
    assert!(rows[0]["os"].get("minimum").is_none());
    assert!(rows[0]["os"].get("maximum").is_none());
    assert_eq!(json, render_json(&inventory)?);

    let markdown = render_markdown(&inventory);
    assert!(markdown.contains("| File | Version | OS support |"));
    assert!(markdown.contains("| DiskUnfold-0.1.0-x64.tar.gz | 0.1.0 | ubuntu 24.04, 26.04 |"));
    assert!(!markdown.contains("24.04–26.04"));
    Ok(())
}

#[test]
fn rejects_ambiguous_empty_and_duplicate_os_version_scopes() -> Result<(), Box<dyn Error>> {
    let ambiguous = serde_json::json!({
        "family": "ubuntu",
        "minimum": "24.04",
        "maximum": "26.04",
        "supported_versions": ["24.04", "26.04"]
    });
    assert!(serde_json::from_value::<OsRange>(ambiguous).is_err());
    let null_bound_conflict = serde_json::json!({
        "family": "ubuntu",
        "maximum": null,
        "supported_versions": ["24.04"]
    });
    assert!(serde_json::from_value::<OsRange>(null_bound_conflict).is_err());

    let staging = TempDir::new()?;
    let mut manifest = sample_manifest(staging.path())?;
    manifest.rows[0].os = OsRange {
        family: OsFamily::Ubuntu,
        versions: OsVersionScope::Exact {
            supported_versions: Vec::new(),
        },
    };
    assert!(matches!(
        validate_and_sort(&mut manifest),
        Err(InventoryError::InvalidField { field: "os", .. })
    ));

    let mut manifest = sample_manifest(staging.path())?;
    manifest.rows[0].os = OsRange {
        family: OsFamily::Ubuntu,
        versions: OsVersionScope::Exact {
            supported_versions: vec![OsVersion::parse("24.04")?, OsVersion::parse("24.04")?],
        },
    };
    assert!(matches!(
        validate_and_sort(&mut manifest),
        Err(InventoryError::InvalidField { field: "os", .. })
    ));
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
    if let OsVersionScope::Range { maximum, .. } = &mut manifest.rows[0].os.versions {
        *maximum = Some(OsVersion::parse("11 24H2")?);
    }
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

#[test]
fn renders_each_artifact_signature_scope_and_escapes_table_delimiters() -> Result<(), Box<dyn Error>>
{
    let staging = TempDir::new()?;
    let mut manifest = sample_manifest(staging.path())?;
    let package = b"macOS package fixture";
    let executable = b"macOS executable fixture";
    let package_name = "DiskUnfold-0.1.0-universal.dmg";
    let executable_name = "DiskUnfold.app";
    let executable_path = "unpacked/macos/DiskUnfold.app";
    let signature_scope = "The app bundle is ad-hoc signed; the DMG is unsigned | not notarized";
    fs::create_dir_all(staging.path().join("unpacked/macos"))?;
    fs::write(staging.path().join(package_name), package)?;
    fs::write(staging.path().join(executable_path), executable)?;

    let mut macos = manifest.rows[0].clone();
    macos.file_name = package_name.into();
    macos.os = OsRange {
        family: OsFamily::Macos,
        versions: OsVersionScope::Range {
            minimum: OsVersion::parse("15")?,
            maximum: None,
        },
    };
    macos.cpu = Cpu::Universal;
    macos.format = PackageFormat::Dmg;
    macos.signature = SignatureMethod::HardenedRuntimeAdHoc;
    macos.signature_scope = signature_scope.into();
    macos.sha256 = Sha256Digest::parse(&sha256(package))?;
    macos.executable_file_name = executable_name.into();
    macos.executable_sha256 = Sha256Digest::parse(&sha256(executable))?;
    macos.run_instructions = "Open the disk image, then launch DiskUnfold.".into();
    macos.bundled_components.clear();
    manifest.rows.push(macos);
    manifest.staged_files.extend([
        StagedFile {
            artifact_file_name: package_name.into(),
            kind: StagedFileKind::Package,
            relative_path: package_name.into(),
        },
        StagedFile {
            artifact_file_name: package_name.into(),
            kind: StagedFileKind::Executable,
            relative_path: executable_path.into(),
        },
    ]);

    let (inventory, _) = prepare_inventory(manifest, staging.path())?;
    let markdown = render_markdown(&inventory);

    assert!(markdown.contains(
        "| File | Version | OS support | CPU | Format | Channel | Signature | Signature scope | SHA-256 | SBOM | Run instructions |"
    ));
    assert!(markdown.contains("| SignPath | Authenticode identity and signed package integrity |"));
    let expected_macos_row = format!(
        "| {package_name} | 0.1.0 | macos 15–open | universal | dmg | direct | Hardened Runtime ad-hoc | The app bundle is ad-hoc signed; the DMG is unsigned \\| not notarized | {} | {package_name}.cdx.json | Open the disk image, then launch DiskUnfold. |",
        sha256(package),
    );
    assert!(markdown.contains(&expected_macos_row));
    assert!(
        inventory
            .rows()
            .iter()
            .all(|row| row.sbom_file_name.as_deref()
                == Some(&format!("{}.cdx.json", row.file_name)[..])),
        "every row names the SBOM that describes it"
    );
    Ok(())
}

#[test]
fn a_fuzz_gate_record_is_published_in_the_json_and_the_markdown()
-> Result<(), Box<dyn std::error::Error>> {
    let inventory = ReleaseInventory {
        product_name: "DiskUnfold".into(),
        version: "0.1.0".into(),
        rows: Vec::new(),
        channel_references: Vec::new(),
        fuzz_gate: None,
    };
    assert!(!render_json(&inventory)?.contains("fuzz_gate"));
    assert!(!render_markdown(&inventory).contains("## Fuzz gate"));

    let record = serde_json::json!({
        "outcome": "no_fuzz_targets_registered", "registered_targets": 0, "open_crashes": 0,
        "latest_run": {"run_id": 9, "url": "https://example/run/9", "conclusion": "success",
            "created_at": "2026-11-13T00:00:00Z", "head_sha": "abc", "same_commit": false},
        "notes": ["no fuzz targets are registered, so no fuzz evidence exists for this release; this is not a fuzz pass"]
    });
    let inventory = inventory.with_fuzz_gate(record);
    let json: serde_json::Value = serde_json::from_str(&render_json(&inventory)?)?;
    assert_eq!(json["fuzz_gate"]["outcome"], "no_fuzz_targets_registered");
    let markdown = render_markdown(&inventory);
    assert!(markdown.contains("## Fuzz gate"));
    assert!(markdown.contains("Registered fuzz targets: 0"));
    assert!(markdown.contains("not a fuzz pass"));
    assert!(markdown.contains("same commit as the release: false"));
    Ok(())
}
