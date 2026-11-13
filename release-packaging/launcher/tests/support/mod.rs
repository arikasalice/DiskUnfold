//! Helpers shared by the launcher CLI integration tests.

#![allow(dead_code)]

use std::{
    fs, io,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_DATA_DIR: AtomicU64 = AtomicU64::new(0);

pub struct TempDataDir(PathBuf);

impl TempDataDir {
    pub fn new() -> io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let sequence = NEXT_DATA_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "diskunfold-cli-test-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDataDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn launcher_with_temp_data() -> io::Result<(Command, TempDataDir)> {
    let data_dir = TempDataDir::new()?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_diskunfold"));
    command.env("DISKUNFOLD_DATA_DIR", data_dir.path());
    let isolated = command.get_envs().any(|(name, value)| {
        name == "DISKUNFOLD_DATA_DIR" && value == Some(data_dir.path().as_os_str())
    });
    if !isolated || !data_dir.path().is_dir() {
        return Err(io::Error::other("isolated CLI data directory not ready"));
    }
    Ok((command, data_dir))
}

#[cfg(windows)]
pub fn closed_pipe_stdio() -> io::Result<std::process::Stdio> {
    let (reader, writer) = io::pipe()?;
    drop(reader);
    Ok(std::process::Stdio::from(writer))
}
