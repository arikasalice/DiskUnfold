//! Disk images attached below a temporary folder, as real mount points.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// An attached image; dropping it detaches the image.
pub struct Attached {
    pub mount: PathBuf,
}

impl Drop for Attached {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil")
            .args(["detach", "-force", "-quiet"])
            .arg(&self.mount)
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
/// names it) at `image` and attaches it at `mount`, without privileges.
pub fn attach(image: &Path, mount: &Path, fs: &str, size: &str) -> Result<Attached, String> {
    run(Command::new("hdiutil")
        .args([
            "create", "-quiet", "-size", size, "-fs", fs, "-volname", "DUMOUNT",
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
    Ok(Attached {
        mount: mount.to_path_buf(),
    })
}
