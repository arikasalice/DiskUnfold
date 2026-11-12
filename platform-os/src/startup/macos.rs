use super::{PrivacySettingsOpenOutcome, StartupDialogOutcome};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSApplication, NSApplicationActivationPolicy, NSWorkspace};
use objc2_foundation::{NSString, NSURL};

const PRIVACY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AllFiles";

pub(super) fn open_privacy_settings() -> PrivacySettingsOpenOutcome {
    let Some(url) = NSURL::URLWithString(&NSString::from_str(PRIVACY_SETTINGS_URL)) else {
        return PrivacySettingsOpenOutcome::Unsupported;
    };

    if NSWorkspace::sharedWorkspace().openURL(&url) {
        PrivacySettingsOpenOutcome::RequestAccepted
    } else {
        PrivacySettingsOpenOutcome::RequestRejected
    }
}

pub(super) fn show_pre_window_error_dialog(title: &str, detail: &str) -> StartupDialogOutcome {
    let Some(main_thread) = MainThreadMarker::new() else {
        return StartupDialogOutcome::Unsupported;
    };
    let application = NSApplication::sharedApplication(main_thread);
    application.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let alert = NSAlert::new(main_thread);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.runModal();

    StartupDialogOutcome::Acknowledged
}
