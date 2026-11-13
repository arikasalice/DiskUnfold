//! Runs the F15 release stages that check generated documents and opened packages against this
//! repository and through the real command line tools.

use du_release_tools::{
    notices::generate,
    sbom::{
        BuildTool, SbomSubject, check_sbom_coverage,
        expected::{expected_components, target_packages},
        render_cyclonedx,
    },
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

fn target_sbom(triple: &'static str) -> Result<String, Box<dyn Error>> {
    let packages = target_packages(&repo(), &[triple])?;
    let targets = [triple];
    let subject = SbomSubject {
        product_name: "DiskUnfold",
        version: "0.1.0",
        file_name: "DiskUnfold-0.1.0.test",
        sha256: &"d".repeat(64),
        rust_targets: &targets,
        bundled: &[],
    };
    let tool = [BuildTool {
        name: "rustc",
        version: "1",
    }];
    Ok(render_cyclonedx(&subject, &packages, &tool)?)
}

#[test]
fn each_target_sbom_matches_the_independent_expected_set_and_the_notices() -> TestResult {
    let notices = generate(&repo())?.text;
    let windows = target_sbom("x86_64-pc-windows-msvc")?;
    let linux = target_sbom("x86_64-unknown-linux-gnu")?;
    for (sbom, triple) in [
        (&windows, "x86_64-pc-windows-msvc"),
        (&linux, "x86_64-unknown-linux-gnu"),
    ] {
        let expected = expected_components(&repo(), &[triple])?;
        check_sbom_coverage(sbom, &notices, Some(&expected))?;
    }
    assert!(
        windows.contains("pkg:cargo/webview2-com@"),
        "the Windows SBOM lists its WebView2 binding"
    );
    assert!(
        !windows.contains("pkg:cargo/gtk4@"),
        "the Windows SBOM does not list gtk4"
    );
    assert!(
        linux.contains("pkg:cargo/gtk4@"),
        "the Linux SBOM lists gtk4"
    );
    assert!(!linux.contains("pkg:cargo/webview2-com@"));
    for sbom in [&windows, &linux] {
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
        assert!(
            sbom.lines()
                .filter(|line| line.contains("\"purl\""))
                .all(|line| !line.contains('+')),
            "purls percent-encode build metadata"
        );
    }
    // The notices cover both targets.
    assert!(notices.contains("cargo gtk4 ") && notices.contains("cargo webview2-com "));
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
fn scan_artifacts_compares_opened_packages_and_reads_the_link_list() -> TestResult {
    let work = TempDir::new()?;
    let mut manifest_row = row("DiskUnfold_0.1.0_amd64.deb", "deb", &"a".repeat(64));
    manifest_row["executable_file_name"] = json!("diskunfold");
    let manifest = work.path().join("merged-manifest.json");
    let document = json!({"product_name": "DiskUnfold", "version": "0.1.0", "rows": [manifest_row],
        "channel_references": [],
        "staged_files": [{"artifact_file_name": "DiskUnfold_0.1.0_amd64.deb", "kind": "executable",
            "relative_path": "unpacked/diskunfold"}]});
    fs::write(&manifest, serde_json::to_vec(&document)?)?;
    let (license, notices) = (work.path().join("LICENSE"), work.path().join("NOTICES"));
    fs::write(&license, "MIT\n")?;
    fs::write(&notices, "notices\n")?;
    let docs = work
        .path()
        .join("extracted/DiskUnfold_0.1.0_amd64.deb/usr/share/doc/diskunfold");
    fs::create_dir_all(&docs)?;
    fs::write(docs.join("LICENSE"), "MIT\n")?;
    fs::write(docs.join("THIRD-PARTY-NOTICES.txt"), "notices\n")?;
    // A 64-bit little-endian ELF header with no program headers: a static executable.
    let staging = work.path().join("staging");
    fs::create_dir_all(staging.join("unpacked"))?;
    let mut elf = vec![0_u8; 64];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[0x20] = 64;
    elf[0x36] = 56;
    fs::write(staging.join("unpacked/diskunfold"), &elf)?;
    let extracted = work.path().join("extracted");
    let args = [
        "scan-artifacts",
        "--manifest",
        path(&manifest)?,
        "--extracted",
        path(&extracted)?,
        "--staging",
        path(&staging)?,
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
    fs::write(docs.join("THIRD-PARTY-NOTICES.txt"), "notices\n")?;

    // A renamed executable that is not an ELF, Mach-O or PE image cannot be read: fail closed.
    fs::write(staging.join("unpacked/diskunfold"), b"#!/bin/sh\n")?;
    let unreadable = run(env!("CARGO_BIN_EXE_release-gate"), &args)?;
    assert_eq!(unreadable.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unreadable.stderr).starts_with("release.license_denied:"));
    Ok(())
}

#[test]
fn check_fuzz_records_the_outcome_and_blocks_on_a_crash_or_a_failed_run() -> TestResult {
    let work = TempDir::new()?;
    let registry = work.path().join("registry.json");
    let issues = work.path().join("issues.json");
    let runs = work.path().join("runs.json");
    let record = work.path().join("fuzz-gate.json");
    fs::write(&registry, "{\"targets\": []}")?;
    fs::write(&issues, "[]")?;
    let run_json = |conclusion: &str| {
        format!(
            "[{{\"databaseId\": 5, \"url\": \"u\", \"conclusion\": \"{conclusion}\", \"createdAt\": \"2026-11-13T00:00:00Z\", \"headSha\": \"abc\"}}]"
        )
    };
    fs::write(&runs, run_json("success"))?;
    let args = [
        "check-fuzz",
        "--issues",
        path(&issues)?,
        "--registry",
        path(&registry)?,
        "--runs",
        path(&runs)?,
        "--now",
        "1794540180",
        "--commit",
        "abc",
        "--require-run",
        "true",
        "--record-out",
        path(&record)?,
    ];
    let ok = run(env!("CARGO_BIN_EXE_release-gate"), &args)?;
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let stdout = String::from_utf8_lossy(&ok.stdout);
    assert!(
        stdout.contains("fuzz_targets=0")
            && stdout.contains("::warning::no fuzz targets are registered"),
        "{stdout}"
    );
    let saved: Value = serde_json::from_str(&fs::read_to_string(&record)?)?;
    assert_eq!(saved["outcome"], "no_fuzz_targets_registered");
    assert_eq!(saved["latest_run"]["same_commit"], true);

    fs::write(&issues, "[{\"number\": 3, \"title\": \"crash\"}]")?;
    let bad = run(env!("CARGO_BIN_EXE_release-gate"), &args)?;
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).starts_with("release.verification_failed:"));

    // No crash file and no issue, but the Fuzz job itself failed: still blocked.
    fs::write(&issues, "[]")?;
    fs::write(&runs, run_json("timed_out"))?;
    let timed_out = run(env!("CARGO_BIN_EXE_release-gate"), &args)?;
    assert_eq!(timed_out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&timed_out.stderr).contains("timed_out"));
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
    assert_eq!(direct["status"], "pending_publication");
    Ok(())
}
