//! Checks that the build-info JSON embedded in the launcher reaches the user.
//!
//! The assertions follow whatever the build embedded, so they hold for a
//! developer build and for a build made with DU_RELEASE_RESOURCES.

use std::{error::Error, path::PathBuf, process::Command};

const EMBEDDED: &str = include_str!(env!("DU_PRODUCT_INFO_PATH"));

fn data_dir() -> Result<PathBuf, Box<dyn Error>> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("diskunfold-embed-{}-{nanos}", std::process::id()));
    std::fs::create_dir(&path)?;
    Ok(path)
}

fn run(args: &[&str]) -> Result<String, Box<dyn Error>> {
    let data = data_dir()?;
    let output = Command::new(env!("CARGO_BIN_EXE_diskunfold"))
        .env("DISKUNFOLD_DATA_DIR", &data)
        .args(args)
        .output()?;
    let _ = std::fs::remove_dir_all(&data);
    if !output.status.success() {
        return Err(format!("{args:?}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[test]
fn embedded_commit_and_kind_are_what_version_reports() -> Result<(), Box<dyn Error>> {
    let info: serde_json::Value = serde_json::from_str(EMBEDDED)?;
    let version = run(&["--lang", "en", "version"])?;
    // A source checkout always embeds the real commit.
    if let Some(commit) = info["build"]["commit"].as_str() {
        assert!(version.contains(commit), "{version}");
    }
    let developer = info["build"]["buildKind"] == "developer";
    assert_eq!(
        developer,
        version.to_lowercase().contains("developer"),
        "{version}"
    );
    Ok(())
}

#[test]
fn embedded_notices_and_support_matrix_are_shown_when_present() -> Result<(), Box<dyn Error>> {
    let info: serde_json::Value = serde_json::from_str(EMBEDDED)?;
    if let Some(text) = info["notices"]["thirdParty"].as_str() {
        let shown = run(&["notices", "THIRD-PARTY-NOTICES"])?;
        assert_eq!(shown.trim_end(), text.trim_end());
    }
    let support = run(&["support", "--format", "json"])?;
    let report: serde_json::Value = serde_json::from_str(&support)?;
    let shown = report["result"]["combinations"]
        .as_array()
        .map_or(0, Vec::len);
    let embedded = info["supportMatrix"]["combinations"]
        .as_array()
        .map_or(0, Vec::len);
    assert_eq!(shown, embedded);
    if let Some(revision) = info["supportMatrix"]["revision"].as_str() {
        assert_eq!(report["result"]["matrixRevision"], revision);
    }
    Ok(())
}
