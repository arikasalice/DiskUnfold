//! Verifies the command line link against an app copied out of the DMG (F15 FR9.HD1: dmg row).
//!
//! Needs a mounted disk image, so it is ignored by default. Run it with
//! `DISKUNFOLD_DMG_APP=/path/to/mounted/DiskUnfold.app cargo test -p du-platform-os --test cli_link_image -- --ignored`.
#![cfg(target_os = "macos")]

use du_platform_os::cli_link::{LINK_NAME, LinkOutcome, install_link};
use std::{path::PathBuf, process::Command};

#[test]
#[ignore = "needs a mounted DMG: set DISKUNFOLD_DMG_APP"]
fn the_link_runs_the_app_copied_from_the_mounted_image() -> Result<(), Box<dyn std::error::Error>> {
    let mounted = PathBuf::from(
        std::env::var_os("DISKUNFOLD_DMG_APP").ok_or("DISKUNFOLD_DMG_APP is not set")?,
    );
    let sandbox = tempfile::tempdir()?;
    // The drag install: the app leaves the image and sits in an Applications folder.
    let applications = sandbox.path().join("Applications");
    std::fs::create_dir(&applications)?;
    let installed = applications.join("DiskUnfold.app");
    let copied = Command::new("ditto")
        .arg(&mounted)
        .arg(&installed)
        .status()?;
    assert!(copied.success());
    let executable = std::fs::canonicalize(installed.join("Contents/MacOS").join(LINK_NAME))?;

    let bin = sandbox.path().join("home/.local/bin");
    assert_eq!(install_link(&executable, &bin)?, LinkOutcome::Created);

    // A shell that sees only the temporary folder resolves and runs the command.
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let output = Command::new("/bin/sh")
        .args(["-c", "command -v diskunfold && diskunfold"])
        .env("PATH", &path)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout)?.lines().next(),
        Some(bin.join(LINK_NAME).to_str().ok_or("non-UTF-8 path")?)
    );
    Ok(())
}
