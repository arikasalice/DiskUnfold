//! Shared fixtures of the inventory tests.

use super::*;
use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::Path};

/// The component record every msi row must carry.
fn webview2_component() -> Result<BundledComponent, Box<dyn Error>> {
    Ok(BundledComponent {
        name: WEBVIEW2_COMPONENT.into(),
        version: "1.3.315.17".into(),
        sha256: Sha256Digest::parse(&sha256(b"webview2"))?,
        signer: "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US"
            .into(),
        source: "https://go.microsoft.com/fwlink/?linkid=2124701".into(),
    })
}

pub(super) fn sample_manifest(root: &Path) -> Result<ReleaseManifest, Box<dyn Error>> {
    fs::write(root.join("DiskUnfold-0.1.0-x64.msi"), b"package")?;
    fs::write(root.join("DiskUnfold.exe"), b"executable")?;
    let row = DistributionRow {
        file_name: "DiskUnfold-0.1.0-x64.msi".into(),
        version: "0.1.0".into(),
        os: OsRange {
            family: OsFamily::Windows,
            versions: OsVersionScope::Range {
                minimum: OsVersion::parse("11 25H2")?,
                maximum: None,
            },
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
        sbom_file_name: None,
        bundled_components: vec![webview2_component()?],
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

pub(super) fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
