use super::{
    StartupDialogOutcome,
    webview2_dialog::{DialogLanguage, DialogUi, show_missing_runtime_dialog},
};
use ::windows::{
    Win32::{
        Globalization::GetUserDefaultUILanguage,
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{
                IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO, MESSAGEBOX_RESULT,
                MessageBoxW, SW_SHOWNORMAL,
            },
        },
    },
    core::{HSTRING, w},
};

/// The native message box and the default browser.
struct NativeUi;

impl DialogUi for NativeUi {
    fn ask_yes_no(&self, title: &str, body: &str) -> Option<bool> {
        let text = HSTRING::from(body);
        let caption = HSTRING::from(title);
        // SAFETY: HSTRING owns both NUL-terminated UTF-16 arguments for this call;
        // the message box has no caller-owned window handle or borrowed buffer.
        let result: MESSAGEBOX_RESULT =
            unsafe { MessageBoxW(None, &text, &caption, MB_YESNO | MB_ICONWARNING) };
        match result.0 {
            0 => None,
            value => Some(value == IDYES.0),
        }
    }

    fn open_in_browser(&self, url: &str) -> bool {
        let target = HSTRING::from(url);
        // SAFETY: every pointer argument is either a NUL-terminated constant, the HSTRING that
        // lives through the call, or absent; ShellExecuteW retains none of them.
        let result = unsafe { ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL) };
        // Values above 32 mean success.
        result.0 as usize > 32
    }
}

fn ui_language() -> DialogLanguage {
    // SAFETY: GetUserDefaultUILanguage takes no arguments and only reads the user's setting.
    DialogLanguage::from_lang_id(unsafe { GetUserDefaultUILanguage() })
}

pub(super) fn show_pre_window_error_dialog(title: &str, detail: &str) -> StartupDialogOutcome {
    if let Some(outcome) = show_missing_runtime_dialog(&NativeUi, ui_language(), detail) {
        return outcome;
    }

    let text = HSTRING::from(detail);
    let caption = HSTRING::from(title);

    // SAFETY: HSTRING owns both NUL-terminated UTF-16 arguments for this call;
    // the message box has no caller-owned window handle or borrowed buffer.
    let result = unsafe { MessageBoxW(None, &text, &caption, MB_OK | MB_ICONERROR) };

    match result.0 {
        1 => StartupDialogOutcome::Acknowledged,
        0 => StartupDialogOutcome::Failed,
        _ => StartupDialogOutcome::Failed,
    }
}
