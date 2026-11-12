use super::{PrivacySettingsOpenOutcome, StartupDialogOutcome};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSApplicationActivationPolicy,
    NSModalResponse, NSModalResponseCancel, NSWorkspace,
};
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
    if !application.setActivationPolicy(NSApplicationActivationPolicy::Regular) {
        return StartupDialogOutcome::Failed;
    }

    let alert = NSAlert::new(main_thread);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    dialog_outcome_from_response(alert.runModal())
}

fn dialog_outcome_from_response(response: NSModalResponse) -> StartupDialogOutcome {
    if response == NSAlertFirstButtonReturn || response == NSModalResponseCancel {
        StartupDialogOutcome::Acknowledged
    } else {
        StartupDialogOutcome::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_app_kit::NSModalResponseAbort;

    #[test]
    fn pre_window_dialog_reports_only_user_dismissals_as_acknowledged() {
        assert_eq!(
            dialog_outcome_from_response(NSAlertFirstButtonReturn),
            StartupDialogOutcome::Acknowledged
        );
        assert_eq!(
            dialog_outcome_from_response(NSModalResponseCancel),
            StartupDialogOutcome::Acknowledged
        );
        assert_eq!(
            dialog_outcome_from_response(NSModalResponseAbort),
            StartupDialogOutcome::Failed
        );
    }
}
