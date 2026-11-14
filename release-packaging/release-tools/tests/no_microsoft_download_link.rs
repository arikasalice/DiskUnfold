//! DiskUnfold does not distribute the WebView2 Runtime, so no published artifact may point to a
//! Microsoft download for it. The startup dialog opens Microsoft's public WebView2 page instead,
//! which is not a direct download and lives only in the application.

use du_release_tools::release_inventory::microsoft_download_reference;
use std::{error::Error, fs, path::PathBuf};

fn repo_file(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn the_files_that_make_published_artifacts_hold_no_microsoft_download_link()
-> Result<(), Box<dyn Error>> {
    for relative in [
        "../windows/package-windows-msi.ps1",
        "../windows/DiskUnfold.wxs",
        "../windows/open-msi-for-scan.ps1",
        "../windows/package-windows-development.ps1",
        "../linux/package-linux.sh",
        "../macos/package-dmg.sh",
        "../release/VERIFY.md",
        "../../.github/workflows/release.yml",
    ] {
        let text = fs::read_to_string(repo_file(relative))?;
        assert_eq!(microsoft_download_reference(&text), None, "{relative}");
    }
    Ok(())
}

#[test]
fn the_webview2_notice_and_license_texts_are_gone_with_the_bundled_installer() {
    for removed in [
        "data/webview2-notice.txt",
        "data/webview2-license.txt",
        "../windows/fetch-webview2.ps1",
    ] {
        assert!(!repo_file(removed).exists(), "{removed} must stay removed");
    }
}

#[test]
fn the_detector_recognises_microsoft_download_links() {
    for link in [
        "https://go.microsoft.com/fwlink/?linkid=2124701",
        "HTTPS://GO.MICROSOFT.COM/FWLINK/?LinkId=1",
        "https://msedge.sf.dl.delivery.mp.microsoft.com/filestreamingservice/files/x",
        "https://www.microsoft.com/download/details.aspx?id=1",
    ] {
        assert!(microsoft_download_reference(link).is_some(), "{link}");
    }
    // Microsoft's public WebView2 page is what the startup dialog opens; it is not a download link.
    assert_eq!(
        microsoft_download_reference("https://developer.microsoft.com/microsoft-edge/webview2/"),
        None
    );
}
