//! Disk images attached for the scan tests, and skips that fail on CI (#43).
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// An attached image; dropping it detaches the image.
pub(super) struct Attached(PathBuf);

impl Drop for Attached {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil")
            .args(["detach", "-force", "-quiet"])
            .arg(&self.0)
            .status();
    }
}

fn run(command: &mut Command) -> Result<(), String> {
    match command.output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// Creates an image of `size` with file system `fs` (as `hdiutil create -fs`
/// names it) at `image` and attaches it at `mount`.
pub(super) fn attach(image: &Path, mount: &Path, fs: &str, size: &str) -> Result<Attached, String> {
    run(Command::new("hdiutil")
        .args([
            "create", "-quiet", "-size", size, "-fs", fs, "-volname", "DUSCAN",
        ])
        .arg(image))?;
    fs::create_dir(mount).map_err(|e| e.to_string())?;
    run(Command::new("hdiutil")
        .args([
            "attach",
            "-quiet",
            "-nobrowse",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(mount)
        .arg(image))?;
    Ok(Attached(mount.to_path_buf()))
}

/// Skips a check the machine cannot run; on CI, where `CI` is set, the skip
/// fails, since a skipped check is otherwise reported as passed.
pub(super) fn skip(reason: &str) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("CI").is_some() {
        return Err(format!("not run on CI: {reason}").into());
    }
    eprintln!("skipped: {reason}");
    Ok(())
}
