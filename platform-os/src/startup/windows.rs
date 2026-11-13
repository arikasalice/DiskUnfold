use super::StartupDialogOutcome;
use ::windows::{
    Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
    core::HSTRING,
};

pub(super) fn show_pre_window_error_dialog(title: &str, detail: &str) -> StartupDialogOutcome {
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
