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

pub(super) fn reason_message(reason: Reason) -> &'static str {
    match reason {
        Reason::InvalidTarget => "folder-drop\n{\"reason\":\"invalid_target\"}",
        Reason::Unsupported => "folder-drop\n{\"reason\":\"unsupported\"}",
    }
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
pub(super) fn response(path: &[u16]) -> String {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::Path};
    let native = OsString::from_wide(path);
    if path.contains(&0) || crate::windows_observation::open_verified(Path::new(&native)).is_err() {
        return reason_message(Reason::InvalidTarget).into();
    }
    path_message(path)
}

fn path_message(path: &[u16]) -> String {
    use std::fmt::Write;
    let mut result = String::from("folder-drop\n{\"path\":\"");
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
            path_message(&[0x43, 0x3a, 0x5c, 0xd800, 0x22, 0xdc00]),
            "folder-drop\n{\"path\":\"\\u0043\\u003a\\u005c\\ud800\\u0022\\udc00\"}"
        );
    }

    #[test]
    fn reports_files_and_missing_paths_as_invalid() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::windows::ffi::OsStrExt;
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("file");
        std::fs::write(&file, b"fixture")?;
        assert_eq!(
            response(&file.as_os_str().encode_wide().collect::<Vec<_>>()),
            reason_message(Reason::InvalidTarget)
        );
        assert_eq!(
            response(
                &dir.path()
                    .join("absent")
                    .as_os_str()
                    .encode_wide()
                    .collect::<Vec<_>>()
            ),
            reason_message(Reason::InvalidTarget)
        );
        assert!(
            response(&dir.path().as_os_str().encode_wide().collect::<Vec<_>>())
                .starts_with("folder-drop\n{\"path\":\"")
        );
        let mut with_nul = dir.path().as_os_str().encode_wide().collect::<Vec<_>>();
        with_nul.extend([0, 65]);
        assert_eq!(response(&with_nul), reason_message(Reason::InvalidTarget));
        let unc: Vec<_> = "\\\\example.invalid\\share\\folder"
            .encode_utf16()
            .collect();
        assert_eq!(response(&unc), reason_message(Reason::InvalidTarget));
        Ok(())
    }
}
