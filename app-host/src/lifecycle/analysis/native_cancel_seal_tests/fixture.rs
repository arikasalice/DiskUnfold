//! Isolated, per-test filesystem paths and their native IPC representations.
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn isolated_tempdir() -> Result<tempfile::TempDir, std::io::Error> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("du-f02-native-seal-");
    #[cfg(unix)]
    {
        builder.tempdir_in("/tmp")
    }
    #[cfg(windows)]
    {
        builder.tempdir()
    }
}

pub(super) fn scope_path(path: &Path) -> Value {
    #[cfg(windows)]
    let (encoding, raw) = {
        use std::os::windows::ffi::OsStrExt;
        let raw = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        ("windows_utf16le", raw)
    };
    #[cfg(unix)]
    let (encoding, raw) = {
        use std::os::unix::ffi::OsStrExt;
        ("unix", path.as_os_str().as_bytes().to_vec())
    };
    json!({
        "encoding": encoding,
        "raw": du_core_contracts::PathBytes::from_bytes(&raw),
        "display": path.to_string_lossy(),
        "lossy": false
    })
}

pub(super) fn native_name(name: &str) -> Vec<u8> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(name)
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    }
    #[cfg(unix)]
    {
        name.as_bytes().to_vec()
    }
}
