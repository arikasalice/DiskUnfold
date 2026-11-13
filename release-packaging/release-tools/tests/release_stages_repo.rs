//! Runs the F15 release stages that check generated documents and opened packages against this
//! repository and through the real command line tools.

use du_release_tools::{
    notices::{generate, shipped_packages},
    sbom::{BuildTool, SbomSubject, check_sbom_matches_notices, render_cyclonedx},
};
use serde_json::{Value, json};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error>>;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(bin: &str, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(bin).args(args).output()?)
}

fn path(value: &Path) -> Result<&str, Box<dyn Error>> {
    value.to_str().ok_or_else(|| "non UTF-8 path".into())
}

#[test]
fn the_repository_sbom_and_notices_list_the_same_packages() -> TestResult {
    let packages = shipped_packages(&repo())?;
    let notices = generate(&repo())?.text;
    let subject = SbomSubject {
        product_name: "DiskUnfold",
        version: "0.1.0",
        file_name: "DiskUnfold-0.1.0.test",
        sha256: &"d".repeat(64),
    };
    let sbom = render_cyclonedx(
        &subject,
        &packages,
        &[BuildTool {
            name: "rustc",
            version: "1",
        }],
    )?;
    check_sbom_matches_notices(&sbom, &notices)?;
    assert!(
        sbom.contains("pkg:cargo/serde@"),
        "cargo production dependencies are listed"
    );
    assert!(
        sbom.contains("pkg:npm/react@"),
        "npm production dependencies are listed"
    );
    assert!(
        !sbom.contains("pkg:cargo/du-"),
        "first-party crates are not third-party components"
    );
    Ok(())
}

fn row(name: &str, format: &str, sha: &str) -> Value {
    json!({
        "file_name": name, "version": "0.1.0",
        "os": {"family": "ubuntu", "supported_versions": ["24.04"]},
        "cpu": "x64", "format": format, "channel": "direct", "signature": "unsigned",
        "signature_scope": "none", "sha256": sha, "executable_file_name": "diskunfold",
        "executable_sha256": sha, "run_instructions": "run"
    })
}

fn manifest(dir: &Path, rows: Vec<Value>) -> Result<PathBuf, Box<dyn Error>> {
    let file = dir.join("merged-manifest.json");
    let document = json!({"product_name": "DiskUnfold", "version": "0.1.0", "rows": rows,
        "channel_references": [], "staged_files": []});
    fs::write(&file, serde_json::to_vec_pretty(&document)?)?;
    Ok(file)
}

#[test]
fn build_sbom_and_check_sboms_agree_and_catch_tampering() -> TestResult {
    let work = TempDir::new()?;
    let manifest = manifest(
        work.path(),
        vec![row(
            "DiskUnfold-0.1.0-x64.tar.gz",
            "tar-gz",
            &"e".repeat(64),
        )],
    )?;
    let notices = work.path().join("THIRD-PARTY-NOTICES");
    fs::write(&notices, generate(&repo())?.text)?;
    let sboms = work.path().join("sbom");
    let built = run(
        env!("CARGO_BIN_EXE_build-sbom"),
        &[path(&manifest)?, path(&repo())?, path(&sboms)?],
    )?;
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(sboms.join("DiskUnfold-0.1.0-x64.tar.gz.cdx.json").is_file());
    assert!(
        fs::read_to_string(sboms.join("SBOM-SHA256SUMS"))?
            .contains("DiskUnfold-0.1.0-x64.tar.gz.cdx.json")
    );

    let args = [
        "check-sboms",
        "--manifest",
        path(&manifest)?,
        "--sbom-dir",
        path(&sboms)?,
        "--notices",
        path(&notices)?,
    ];
    let ok = run(env!("CARGO_BIN_EXE_release-gate"), &args)?;
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );

    // A package whose SHA-256 the SBOM does not describe fails with the design key.
    let other = self::manifest(
        work.path(),
        vec![row(
            "DiskUnfold-0.1.0-x64.tar.gz",
            "tar-gz",
            &"f".repeat(64),
        )],
    )?;
    let args = [
        "check-sboms",
        "--manifest",
        path(&other)?,
        "--sbom-dir",
        path(&sboms)?,
        "--notices",
        path(&notices)?,
    ];
    let bad = run(env!("CARGO_BIN_EXE_release-gate"), &args)?;
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).starts_with("release.notice_mismatch:"));

    // A missing SBOM fails too.
    fs::remove_file(sboms.join("DiskUnfold-0.1.0-x64.tar.gz.cdx.json"))?;
    let args = [
        "check-sboms",
        "--manifest",
        path(&manifest)?,
        "--sbom-dir",
        path(&sboms)?,
        "--notices",
        path(&notices)?,
    ];
    assert_eq!(
        run(env!("CARGO_BIN_EXE_release-gate"), &args)?
            .status
            .code(),
        Some(1)
    );
    Ok(())
}

#[test]
fn scan_artifacts_compares_opened_packages_byte_for_byte() -> TestResult {
    let work = TempDir::new()?;
    let manifest = manifest(
        work.path(),
        vec![row("DiskUnfold_0.1.0_amd64.deb", "deb", &"a".repeat(64))],
    )?;
    let (license, notices) = (work.path().join("LICENSE"), work.path().join("NOTICES"));
    fs::write(&license, "MIT\n")?;
    fs::write(&notices, "notices\n")?;
    let docs = work
        .path()
        .join("extracted/DiskUnfold_0.1.0_amd64.deb/usr/share/doc/diskunfold");
    fs::create_dir_all(&docs)?;
    fs::write(docs.join("LICENSE"), "MIT\n")?;
    fs::write(docs.join("THIRD-PARTY-NOTICES.txt"), "notices\n")?;
    let extracted = work.path().join("extracted");
    let args = [
        "scan-artifacts",
        "--manifest",
        path(&manifest)?,
        "--extracted",
        path(&extracted)?,
        "--license",
        path(&license)?,
        "--notices",
        path(&notices)?,
    ];
    let ok = run(env!("CARGO_BIN_EXE_release-gate"), &args)?;
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(String::from_utf8_lossy(&ok.stdout).contains("packages_scanned=1"));

    fs::write(docs.join("THIRD-PARTY-NOTICES.txt"), "notices")?;
    let bad = run(env!("CARGO_BIN_EXE_release-gate"), &args)?;
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).starts_with("release.notice_mismatch:"));
    Ok(())
}

#[test]
fn check_fuzz_blocks_on_an_open_crash_issue() -> TestResult {
    let work = TempDir::new()?;
    let registry = work.path().join("registry.json");
    let issues = work.path().join("issues.json");
    fs::write(&registry, "{\"targets\": []}")?;
    fs::write(&issues, "[]")?;
    let args = [
        "check-fuzz",
        "--issues",
        path(&issues)?,
        "--registry",
        path(&registry)?,
    ];
    let ok = run(env!("CARGO_BIN_EXE_release-gate"), &args)?;
    assert!(ok.status.success());
    assert!(String::from_utf8_lossy(&ok.stdout).contains("fuzz_targets=0"));
    fs::write(&issues, "[{\"number\": 3, \"title\": \"crash\"}]")?;
    let bad = run(env!("CARGO_BIN_EXE_release-gate"), &args)?;
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).starts_with("release.verification_failed:"));
    Ok(())
}

#[test]
fn build_channel_manifests_writes_the_three_channel_files() -> TestResult {
    let work = TempDir::new()?;
    let inventory = work.path().join("release-inventory.json");
    let document = json!({"product_name": "DiskUnfold", "version": "0.1.0",
        "rows": [row("DiskUnfold_0.1.0_amd64.deb", "deb", &"a".repeat(64))], "channel_references": []});
    fs::write(&inventory, serde_json::to_vec(&document)?)?;
    let out = work.path().join("channels");
    let done = run(
        env!("CARGO_BIN_EXE_build-channel-manifests"),
        &[
            path(&inventory)?,
            path(&out)?,
            "--repository",
            "owner/repo",
            "--tag",
            "v0.1.0",
        ],
    )?;
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    for name in [
        "channel-direct.json",
        "channel-winget.json",
        "channel-store.json",
    ] {
        assert!(out.join(name).is_file(), "{name}");
    }
    let direct: Value =
        serde_json::from_str(&fs::read_to_string(out.join("channel-direct.json"))?)?;
    assert_eq!(direct["status"], "available");
    Ok(())
}
