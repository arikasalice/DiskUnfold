//! Translate WebView2 file objects without losing Windows UTF-16 code units.
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2File, ICoreWebView2WebMessageReceivedEventArgs,
    ICoreWebView2WebMessageReceivedEventArgs2,
};
use windows::{
    Win32::System::Com::CoTaskMemFree,
    core::{Interface, PWSTR},
};

#[derive(Clone, Copy)]
pub(super) enum Reason {
    InvalidTarget,
    Unsupported,
}

pub(super) fn reason_message(id: u64, reason: Reason) -> String {
    let key = match reason {
        Reason::InvalidTarget => "invalid_target",
        Reason::Unsupported => "unsupported",
    };
    format!("folder-drop:{id}\n{{\"reason\":\"{key}\"}}")
}

/// Extracts exactly one DOM File's OS path; this does not access file contents.
pub(super) unsafe fn path(
    args: &ICoreWebView2WebMessageReceivedEventArgs,
) -> Result<Vec<u16>, Reason> {
    // SAFETY: This function is called on the owning STA during the message callback.
    unsafe {
        let args = args
            .cast::<ICoreWebView2WebMessageReceivedEventArgs2>()
            .map_err(|_| Reason::Unsupported)?;
        let objects = args.AdditionalObjects().map_err(|_| Reason::Unsupported)?;
        let mut count = 0;
        objects.Count(&mut count).map_err(|_| Reason::Unsupported)?;
        if count != 1 {
            return Err(Reason::InvalidTarget);
        }
        let file = objects
            .GetValueAtIndex(0)
            .and_then(|object| object.cast::<ICoreWebView2File>())
            .map_err(|_| Reason::Unsupported)?;
        let mut raw = PWSTR::null();
        file.Path(&mut raw).map_err(|_| Reason::Unsupported)?;
        if raw.is_null() {
            return Err(Reason::InvalidTarget);
        }
        let value = raw.as_wide().to_vec();
        CoTaskMemFree(Some(raw.0.cast()));
        if value.is_empty() || value.len() > 32767 {
            return Err(Reason::InvalidTarget);
        }
        Ok(value)
    }
}

/// Applies the local directory boundary before presenting the selected path.
pub(super) fn response(id: u64, path: &[u16]) -> String {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::Path};
    let native = OsString::from_wide(path);
    if path.contains(&0) || crate::windows_observation::open_verified(Path::new(&native)).is_err() {
        return reason_message(id, Reason::InvalidTarget);
    }
    path_message(id, path)
}

fn path_message(id: u64, path: &[u16]) -> String {
    use std::fmt::Write;
    let mut result = format!("folder-drop:{id}\n{{\"path\":\"");
    for unit in path {
        let _ = write!(result, "\\u{unit:04x}");
    }
    result.push_str("\"}");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_raw_utf16_including_unpaired_surrogates() {
        assert_eq!(
            path_message(42, &[0x43, 0x3a, 0x5c, 0xd800, 0x22, 0xdc00]),
            "folder-drop:42\n{\"path\":\"\\u0043\\u003a\\u005c\\ud800\\u0022\\udc00\"}"
        );
    }

    #[test]
    fn reports_files_and_missing_paths_as_invalid() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::windows::ffi::OsStrExt;
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("file");
        std::fs::write(&file, b"fixture")?;
        let id = 43;
        let rejected = reason_message(id, Reason::InvalidTarget);
        assert_eq!(
            response(id, &file.as_os_str().encode_wide().collect::<Vec<_>>()),
            rejected
        );
        assert_eq!(
            response(
                id,
                &dir.path()
                    .join("absent")
                    .as_os_str()
                    .encode_wide()
                    .collect::<Vec<_>>()
            ),
            rejected
        );
        assert!(
            response(
                id,
                &dir.path().as_os_str().encode_wide().collect::<Vec<_>>()
            )
            .starts_with("folder-drop:43\n{\"path\":\"")
        );
        let mut with_nul = dir.path().as_os_str().encode_wide().collect::<Vec<_>>();
        with_nul.extend([0, 65]);
        assert_eq!(response(id, &with_nul), rejected);
        let unc: Vec<_> = "\\\\example.invalid\\share\\folder"
            .encode_utf16()
            .collect();
        assert_eq!(response(id, &unc), rejected);
        Ok(())
    }
}
