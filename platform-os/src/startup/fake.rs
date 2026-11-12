use super::{
    FullDiskAccessStatus, PrivacySettingsOpenOutcome, StartupCapabilitiesPort,
    StartupDialogOutcome, WebViewAvailability,
};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Deterministic startup capabilities for isolated tests.
///
/// The privacy settings method only records a request and never opens a UI.
pub struct FakeStartupCapabilities {
    full_disk_access: FullDiskAccessStatus,
    webview: WebViewAvailability,
    privacy_settings_outcome: PrivacySettingsOpenOutcome,
    startup_dialog_outcome: StartupDialogOutcome,
    privacy_settings_requests: AtomicUsize,
    startup_dialog_requests: AtomicUsize,
}

impl FakeStartupCapabilities {
    /// Creates a fake adapter with fixed observations.
    pub fn new(
        full_disk_access: FullDiskAccessStatus,
        webview: WebViewAvailability,
        privacy_settings_outcome: PrivacySettingsOpenOutcome,
        startup_dialog_outcome: StartupDialogOutcome,
    ) -> Self {
        Self {
            full_disk_access,
            webview,
            privacy_settings_outcome,
            startup_dialog_outcome,
            privacy_settings_requests: AtomicUsize::new(0),
            startup_dialog_requests: AtomicUsize::new(0),
        }
    }

    /// Returns the number of simulated explicit settings-open requests.
    pub fn privacy_settings_requests(&self) -> usize {
        self.privacy_settings_requests.load(Ordering::Relaxed)
    }

    /// Returns the number of simulated startup-dialog requests.
    pub fn startup_dialog_requests(&self) -> usize {
        self.startup_dialog_requests.load(Ordering::Relaxed)
    }
}

impl StartupCapabilitiesPort for FakeStartupCapabilities {
    fn full_disk_access_status(&self) -> FullDiskAccessStatus {
        self.full_disk_access
    }

    fn webview_availability(&self) -> WebViewAvailability {
        self.webview
    }

    fn open_privacy_settings(&self) -> PrivacySettingsOpenOutcome {
        self.privacy_settings_requests
            .fetch_add(1, Ordering::Relaxed);
        self.privacy_settings_outcome
    }

    fn show_pre_window_error_dialog(&self, _title: &str, _detail: &str) -> StartupDialogOutcome {
        self.startup_dialog_requests.fetch_add(1, Ordering::Relaxed);
        self.startup_dialog_outcome
    }
}
