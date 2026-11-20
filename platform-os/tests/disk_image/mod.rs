//! Disk images attached below a temporary folder, as real mount points.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
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
        Ok(output) => Err(format!(
            "{} {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Err(error.to_string()),
    }
}

/// Runs `step` up to five times a second apart: on CI runners `hdiutil` now and
/// then fails with no message and succeeds on the next try (as detach did, #252).
fn retried(mut step: impl FnMut() -> Result<(), String>) -> Result<(), String> {
    let mut failures = Vec::new();
    for attempt in 0..5 {
        if attempt > 0 {
            thread::sleep(Duration::from_secs(1));
        }
        match step() {
            Ok(()) => return Ok(()),
            Err(reason) => failures.push(reason),
        }
    }
    Err(failures.join("; "))
}

/// Creates an image of `size` with file system `fs` (as `hdiutil create -fs`
/// names it) at `image` and attaches it at `mount`, without privileges.
pub fn attach(image: &Path, mount: &Path, fs: &str, size: &str) -> Result<Attached, String> {
    retried(|| {
        // A failed create may leave a partial image that the next create refuses.
        let _ = fs::remove_file(image);
        run(Command::new("hdiutil")
            .args([
                "create", "-quiet", "-size", size, "-fs", fs, "-volname", "DUMOUNT",
            ])
            .arg(image))
    })?;
    fs::create_dir(mount).map_err(|e| e.to_string())?;
    retried(|| {
        run(Command::new("hdiutil")
            .args([
                "attach",
                "-quiet",
                "-nobrowse",
                "-noautoopen",
                "-mountpoint",
            ])
            .arg(mount)
            .arg(image))
    })?;
    Ok(Attached {
        mount: mount.to_path_buf(),
    })
}
