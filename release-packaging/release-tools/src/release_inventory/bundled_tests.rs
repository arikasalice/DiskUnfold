//! Bundled-component records on msi rows.

use super::fixtures::sample_manifest;
use super::*;
use std::error::Error;
use tempfile::TempDir;

#[test]
fn msi_rows_must_record_the_webview2_installer_signed_by_microsoft() -> Result<(), Box<dyn Error>> {
    let staging = TempDir::new()?;
    let valid = sample_manifest(staging.path())?;
    let (inventory, _) = prepare_inventory(valid.clone(), staging.path())?;
    let markdown = render_markdown(&inventory);
    assert!(markdown.contains("## Bundled components"));
    assert!(markdown.contains("| DiskUnfold-0.1.0-x64.msi | Microsoft Edge WebView2 Runtime Evergreen Standalone Installer | 1.3.315.17 |"));
    assert!(render_json(&inventory)?.contains("bundled_components"));

    let mut missing = valid.clone();
    missing.rows[0].bundled_components.clear();
    assert!(prepare_inventory(missing, staging.path()).is_err());

    let mut other_signer = valid.clone();
    other_signer.rows[0].bundled_components[0].signer = "CN=Someone Else".into();
    assert!(prepare_inventory(other_signer, staging.path()).is_err());

    let mut lookalike = valid.clone();
    lookalike.rows[0].bundled_components[0].signer = "CN=Microsoft Corporation Fake, O=Evil".into();
    assert!(prepare_inventory(lookalike, staging.path()).is_err());

    let mut extra = valid.clone();
    let component = extra.rows[0].bundled_components[0].clone();
    extra.rows[0].bundled_components.push(component);
    assert!(prepare_inventory(extra, staging.path()).is_err());

    let mut wrong_format = valid;
    wrong_format.rows[0].format = PackageFormat::Deb;
    assert!(prepare_inventory(wrong_format, staging.path()).is_err());
    Ok(())
}
