use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    error::Error,
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

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

fn write_manifest(root: &Path, bad_package_digest: bool) -> Result<(), Box<dyn Error>> {
    let package = b"synthetic package fixture";
    let executable = b"synthetic executable fixture";
    fs::write(root.join("DiskUnfold-0.0.1-x64.msi"), package)?;
    fs::write(root.join("DiskUnfold.exe"), executable)?;
    let package_digest = if bad_package_digest {
        "0".repeat(64)
    } else {
        sha256(package)
    };
    let manifest = json!({
        "product_name": "DiskUnfold",
        "version": "0.0.1",
        "rows": [{
            "file_name": "DiskUnfold-0.0.1-x64.msi",
            "version": "0.0.1",
            "os": {"family": "windows", "minimum": "11 25H2", "maximum": null},
            "cpu": "x64",
            "format": "msi",
            "channel": "direct",
            "signature": "unsigned",
            "signature_scope": "synthetic fixture; signature not verified",
            "sha256": package_digest,
            "executable_file_name": "DiskUnfold.exe",
            "executable_sha256": sha256(executable),
            "run_instructions": "Install the package, then launch DiskUnfold.",
        }],
        "channel_references": [{
            "channel": "winget",
            "version": "0.0.1",
            "cpu": "x64",
            "artifact_file_name": "DiskUnfold-0.0.1-x64.msi",
            "update_method": "channel_command"
        }],
        "staged_files": [
            {"artifact_file_name": "DiskUnfold-0.0.1-x64.msi", "kind": "package", "relative_path": "DiskUnfold-0.0.1-x64.msi"},
            {"artifact_file_name": "DiskUnfold-0.0.1-x64.msi", "kind": "executable", "relative_path": "DiskUnfold.exe"}
        ]
    });
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

fn write_multi_distribution_manifest(root: &Path) -> Result<(), Box<dyn Error>> {
    let windows_package = "DiskUnfold-0.0.1-x64.msi";
    let macos_package = "DiskUnfold-0.0.1-universal.dmg";
    let windows_executable = b"windows executable fixture";
    let macos_executable = b"macos executable fixture";
    fs::create_dir_all(root.join("unpacked/windows-x64"))?;
    fs::create_dir_all(root.join("unpacked/macos-universal"))?;
    fs::write(root.join(windows_package), b"windows package fixture")?;
    fs::write(root.join(macos_package), b"macOS package fixture")?;
    fs::write(
        root.join("unpacked/windows-x64/DiskUnfold.exe"),
        windows_executable,
    )?;
    fs::write(
        root.join("unpacked/macos-universal/DiskUnfold.exe"),
        macos_executable,
    )?;
    let manifest = json!({
        "product_name": "DiskUnfold",
        "version": "0.0.1",
        "rows": [
            {
                "file_name": windows_package,
                "version": "0.0.1",
                "os": {"family": "windows", "minimum": "11 25H2", "maximum": null},
                "cpu": "x64",
                "format": "msi",
                "channel": "direct",
                "signature": "unsigned",
                "signature_scope": "synthetic fixture; signature not verified",
                "sha256": sha256(b"windows package fixture"),
                "executable_file_name": "DiskUnfold.exe",
                "executable_sha256": sha256(windows_executable),
                "run_instructions": "Install the MSI, then launch DiskUnfold.",
            },
            {
                "file_name": macos_package,
                "version": "0.0.1",
                "os": {"family": "macos", "minimum": "15", "maximum": null},
                "cpu": "universal",
                "format": "dmg",
                "channel": "direct",
                "signature": "unsigned",
                "signature_scope": "synthetic fixture; signature not verified",
                "sha256": sha256(b"macOS package fixture"),
                "executable_file_name": "DiskUnfold.exe",
                "executable_sha256": sha256(macos_executable),
                "run_instructions": "Open the disk image, then launch DiskUnfold."
            }
        ],
        "channel_references": [],
        "staged_files": [
            {"artifact_file_name": windows_package, "kind": "package", "relative_path": windows_package},
            {"artifact_file_name": windows_package, "kind": "executable", "relative_path": "unpacked/windows-x64/DiskUnfold.exe"},
            {"artifact_file_name": macos_package, "kind": "package", "relative_path": macos_package},
            {"artifact_file_name": macos_package, "kind": "executable", "relative_path": "unpacked/macos-universal/DiskUnfold.exe"}
        ]
    });
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

fn run_tool(manifest: &Path, staging: &Path, output: &Path) -> Result<Output, std::io::Error> {
    Command::new(env!("CARGO_BIN_EXE_build-release-inventory"))
        .arg(manifest)
        .arg(staging)
        .arg(output)
        .output()
}

#[test]
fn cli_generates_local_inventory_and_rejects_digest_mismatch_before_writing()
-> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let staging = temporary.path().join("staging");
    let output = temporary.path().join("output");
    fs::create_dir(&staging)?;
    write_manifest(&staging, false)?;

    let success = run_tool(&staging.join("manifest.json"), &staging, &output)?;
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    let inventory: Value =
        serde_json::from_slice(&fs::read(output.join("release-inventory.json"))?)?;
    assert_eq!(inventory["rows"].as_array().map(Vec::len), Some(1));
    assert!(
        fs::read_to_string(output.join("DISTRIBUTION.md"))?.contains("Internal inventory output")
    );
    let checksums = fs::read_to_string(output.join("SHA256SUMS"))?;
    assert!(checksums.contains("DiskUnfold-0.0.1-x64.msi"));
    assert!(!checksums.contains("DiskUnfold.exe"));
    let executable_checksums: Value =
        serde_json::from_slice(&fs::read(output.join("EXECUTABLE-SHA256.json"))?)?;
    assert_eq!(
        executable_checksums["executables"][0]["artifact_file_name"],
        "DiskUnfold-0.0.1-x64.msi"
    );
    assert_eq!(
        executable_checksums["executables"][0]["executable_file_name"],
        "DiskUnfold.exe"
    );
    let distribution = fs::read_to_string(output.join("DISTRIBUTION.md"))?;
    assert!(distribution.contains("not verified or created by this tool"));
    assert!(distribution.contains("not a release gate"));
    for entry in fs::read_dir(&output)? {
        let path = entry?.path();
        let published = fs::read_to_string(&path)?;
        assert_eq!(
            du_release_tools::release_inventory::microsoft_download_reference(&published),
            None,
            "{}",
            path.display()
        );
    }

    let bad_staging = temporary.path().join("bad-staging");
    let bad_output = temporary.path().join("bad-output");
    fs::create_dir(&bad_staging)?;
    write_manifest(&bad_staging, true)?;
    let failure = run_tool(
        &bad_staging.join("manifest.json"),
        &bad_staging,
        &bad_output,
    )?;
    assert!(!failure.status.success());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("SHA-256 mismatch"));
    assert!(!bad_output.exists());
    Ok(())
}

#[test]
fn cli_emits_verifiable_package_sums_and_package_scoped_executable_hashes()
-> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let staging = temporary.path().join("staging");
    let output = temporary.path().join("output");
    let release_assets = temporary.path().join("release-assets");
    fs::create_dir(&staging)?;
    write_multi_distribution_manifest(&staging)?;

    let generated = run_tool(&staging.join("manifest.json"), &staging, &output)?;
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let sums = fs::read_to_string(output.join("SHA256SUMS"))?;
    assert_eq!(sums.lines().count(), 2);
    assert!(!sums.contains("DiskUnfold.exe"));
    let executable_sums: Value =
        serde_json::from_slice(&fs::read(output.join("EXECUTABLE-SHA256.json"))?)?;
    let executables = executable_sums["executables"]
        .as_array()
        .ok_or("executable checksum list is missing")?;
    assert_eq!(executables.len(), 2);
    assert_eq!(
        executables
            .iter()
            .map(|entry| entry["sha256"].as_str().unwrap_or_default().to_owned())
            .collect::<BTreeSet<_>>()
            .len(),
        2,
        "same-basename executables must retain their distinct package hashes"
    );
    let expected_executables = [
        (
            "DiskUnfold-0.0.1-x64.msi",
            sha256(&fs::read(
                staging.join("unpacked/windows-x64/DiskUnfold.exe"),
            )?),
        ),
        (
            "DiskUnfold-0.0.1-universal.dmg",
            sha256(&fs::read(
                staging.join("unpacked/macos-universal/DiskUnfold.exe"),
            )?),
        ),
    ];
    for (artifact, digest) in expected_executables {
        assert!(
            executables.iter().any(|entry| {
                entry["artifact_file_name"] == artifact && entry["sha256"] == digest
            })
        );
    }
    let distribution = fs::read_to_string(output.join("DISTRIBUTION.md"))?;
    assert!(distribution.contains("Get-FileHash"));
    assert!(distribution.contains("shasum -a 256 '<package-file>'"));
    assert!(distribution.contains("sha256sum '<package-file>'"));
    assert!(distribution.contains("shasum -a 256 -c SHA256SUMS"));
    assert!(distribution.contains("sha256sum -c SHA256SUMS"));
    assert!(distribution.contains("EXECUTABLE-SHA256.json"));
    assert!(distribution.contains("Match means"));
    assert!(distribution.contains("Mismatch means"));
    assert!(distribution.contains("unavailable"));

    // A published asset directory is flat: package files and SHA256SUMS share this directory.
    fs::create_dir(&release_assets)?;
    for name in ["DiskUnfold-0.0.1-x64.msi", "DiskUnfold-0.0.1-universal.dmg"] {
        fs::copy(staging.join(name), release_assets.join(name))?;
    }
    fs::copy(output.join("SHA256SUMS"), release_assets.join("SHA256SUMS"))?;
    fs::copy(
        output.join("EXECUTABLE-SHA256.json"),
        release_assets.join("EXECUTABLE-SHA256.json"),
    )?;

    let verifier = Command::new("sha256sum")
        .args(["-c", "SHA256SUMS"])
        .current_dir(&release_assets)
        .output()
        .or_else(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Command::new("shasum")
                    .args(["-a", "256", "-c", "SHA256SUMS"])
                    .current_dir(&release_assets)
                    .output()
            } else {
                Err(error)
            }
        })?;
    assert!(
        verifier.status.success(),
        "checksum verification failed: {}{}",
        String::from_utf8_lossy(&verifier.stdout),
        String::from_utf8_lossy(&verifier.stderr)
    );

    Ok(())
}

#[test]
fn cli_keeps_nonconsecutive_ubuntu_versions_on_one_asset_row() -> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let staging = temporary.path().join("staging");
    let output = temporary.path().join("output");
    fs::create_dir(&staging)?;
    let package_name = "DiskUnfold-0.0.1-x64.tar.gz";
    let executable_name = "diskunfold";
    let package = b"Ubuntu tarball fixture";
    let executable = b"Ubuntu executable fixture";
    fs::write(staging.join(package_name), package)?;
    fs::write(staging.join(executable_name), executable)?;
    let manifest = json!({
        "product_name": "DiskUnfold",
        "version": "0.0.1",
        "rows": [{
            "file_name": package_name,
            "version": "0.0.1",
            "os": {
                "family": "ubuntu",
                "supported_versions": ["26.04", "24.04"]
            },
            "cpu": "x64",
            "format": "tar-gz",
            "channel": "direct",
            "signature": "unsigned",
            "signature_scope": "No package signature; compare the published digest and provenance.",
            "sha256": sha256(package),
            "executable_file_name": executable_name,
            "executable_sha256": sha256(executable),
            "run_instructions": "Extract the archive and run diskunfold."
        }],
        "channel_references": [],
        "staged_files": [
            {"artifact_file_name": package_name, "kind": "package", "relative_path": package_name},
            {"artifact_file_name": package_name, "kind": "executable", "relative_path": executable_name}
        ]
    });
    let manifest_path = staging.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;

    let generated = run_tool(&manifest_path, &staging, &output)?;
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let inventory: Value =
        serde_json::from_slice(&fs::read(output.join("release-inventory.json"))?)?;
    let rows = inventory["rows"]
        .as_array()
        .ok_or("inventory rows are missing")?;
    assert_eq!(rows.len(), 1, "the package filename is unique for this CPU");
    assert_eq!(
        rows[0]["os"]["supported_versions"],
        json!(["24.04", "26.04"])
    );
    assert!(rows[0]["os"].get("minimum").is_none());
    assert!(rows[0]["os"].get("maximum").is_none());
    let distribution = fs::read_to_string(output.join("DISTRIBUTION.md"))?;
    assert!(distribution.contains("| ubuntu 24.04, 26.04 |"));
    assert!(!distribution.contains("24.04–26.04"));
    assert_eq!(
        fs::read_to_string(output.join("SHA256SUMS"))?
            .lines()
            .count(),
        1
    );
    Ok(())
}
