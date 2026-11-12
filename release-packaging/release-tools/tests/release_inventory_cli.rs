use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
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
    fs::write(root.join("DiskUnfold-0.1.0-x64.msi"), package)?;
    fs::write(root.join("DiskUnfold.exe"), executable)?;
    let package_digest = if bad_package_digest {
        "0".repeat(64)
    } else {
        sha256(package)
    };
    let manifest = json!({
        "product_name": "DiskUnfold",
        "version": "0.1.0",
        "rows": [{
            "file_name": "DiskUnfold-0.1.0-x64.msi",
            "version": "0.1.0",
            "os": {"family": "windows", "minimum": "11 25H2", "maximum": null},
            "cpu": "x64",
            "format": "msi",
            "channel": "direct",
            "signature": "unsigned",
            "signature_scope": "synthetic fixture; signature not verified",
            "sha256": package_digest,
            "executable_file_name": "DiskUnfold.exe",
            "executable_sha256": sha256(executable),
            "run_instructions": "Install the package, then launch DiskUnfold."
        }],
        "channel_references": [{
            "channel": "winget",
            "version": "0.1.0",
            "cpu": "x64",
            "artifact_file_name": "DiskUnfold-0.1.0-x64.msi",
            "update_method": "channel_command"
        }],
        "staged_files": [
            {"artifact_file_name": "DiskUnfold-0.1.0-x64.msi", "kind": "package", "relative_path": "DiskUnfold-0.1.0-x64.msi"},
            {"artifact_file_name": "DiskUnfold-0.1.0-x64.msi", "kind": "executable", "relative_path": "DiskUnfold.exe"}
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
    assert!(checksums.contains("DiskUnfold-0.1.0-x64.msi"));
    assert!(checksums.contains("DiskUnfold.exe"));

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
