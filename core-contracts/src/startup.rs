//! Startup-time operating-system capability contracts.

/// The application's observed full disk access state.
///
/// `Unknown` must allow startup to continue. An adapter may return `Denied`
/// only when an operating-system interface directly establishes that result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullDiskAccessStatus {
    /// The operating system directly confirms full disk access is granted.
    Granted,
    /// The operating system directly confirms full disk access is denied.
    Denied,
    /// The adapter cannot reliably determine the grant state.
    Unknown,
    /// Full disk access is not a permission model on this platform.
    NotApplicable,
}

/// Result of checking webview prerequisites without opening a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebViewAvailability {
    /// The platform adapter has reliable evidence the webview can be used.
    Available,
    /// This build does not include a webview adapter.
    Unavailable,
    /// Build or library presence alone does not prove a usable webview session.
    Unknown,
}

/// Outcome of an explicit request to open the platform privacy settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivacySettingsOpenOutcome {
    /// The adapter has native evidence the requested settings screen appeared.
    ConfirmedOpened,
    /// The OS accepted the URL launch request, but the screen's visibility is unknown.
    RequestAccepted,
    /// The OS rejected the launch request.
    RequestRejected,
    /// This adapter or build does not support the requested settings screen.
    Unsupported,
    /// This privacy settings flow does not apply to the platform.
    NotApplicable,
}

/// Outcome of displaying an explicit blocking startup error dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupDialogOutcome {
    /// The native dialog was displayed and the user dismissed it.
    Acknowledged,
    /// The build or desktop session cannot display this native dialog.
    Unsupported,
    /// The native dialog could not be displayed or completed.
    Failed,
}

/// Supplies startup-only OS observations and explicit privacy-settings actions.
///
/// Reading capabilities must not prompt the user or open a system settings UI.
/// The settings action runs only after an explicit call from the host.
pub trait StartupCapabilitiesPort: Send + Sync {
    /// Returns the best available full disk access observation.
    fn full_disk_access_status(&self) -> FullDiskAccessStatus;

    /// Reports whether the compiled webview can be used without opening it.
    fn webview_availability(&self) -> WebViewAvailability;

    /// Requests that the platform open its privacy settings screen.
    ///
    /// `RequestAccepted` does not prove that the settings window became visible
    /// or that access changed. An unsupported capability is returned as a value.
    fn open_privacy_settings(&self) -> PrivacySettingsOpenOutcome;

    /// Displays a blocking error before the app creates its webview.
    ///
    /// The caller owns the message content; the adapter displays it as supplied.
    /// On macOS and Linux this method must be called on the GUI main thread. It
    /// blocks until the user dismisses the dialog or the platform reports
    /// that a native dialog cannot be displayed.
    fn show_pre_window_error_dialog(&self, title: &str, detail: &str) -> StartupDialogOutcome;
}
