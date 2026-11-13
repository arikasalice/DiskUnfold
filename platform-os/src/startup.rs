//! Startup-time operating-system capabilities used by the F15 host checks.

pub use du_core_contracts::startup::{
    FullDiskAccessStatus, PrivacySettingsOpenOutcome, StartupCapabilitiesPort,
    StartupDialogOutcome, WebViewAvailability,
};

#[cfg(feature = "fake")]
mod fake;
#[cfg(any(target_os = "macos", test))]
mod fda;
#[cfg(all(target_os = "linux", feature = "webview"))]
mod linux;
#[cfg(all(target_os = "macos", feature = "webview"))]
mod macos;
#[cfg(all(target_os = "macos", feature = "webview"))]
mod webkit_framework;
#[cfg(all(target_os = "linux", feature = "webview"))]
#[allow(unsafe_code)]
mod webkit_gtk;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod webview2;
#[cfg(any(
    target_os = "windows",
    all(any(target_os = "linux", target_os = "macos"), feature = "webview"),
    test
))]
mod webview_runtime;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod windows;

#[cfg(feature = "fake")]
pub use fake::FakeStartupCapabilities;

/// Production startup capability adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemStartupCapabilities;

impl StartupCapabilitiesPort for SystemStartupCapabilities {
    fn full_disk_access_status(&self) -> FullDiskAccessStatus {
        #[cfg(target_os = "macos")]
        {
            // Read-only open attempts on FDA-protected paths; unknown
            // evidence never reports Granted.
            fda::full_disk_access_status()
        }
        #[cfg(not(target_os = "macos"))]
        {
            FullDiskAccessStatus::NotApplicable
        }
    }

    fn webview_availability(&self) -> WebViewAvailability {
        if cfg!(feature = "webview") {
            runtime_availability()
        } else {
            WebViewAvailability::Unavailable
        }
    }

    fn open_privacy_settings(&self) -> PrivacySettingsOpenOutcome {
        #[cfg(all(target_os = "macos", feature = "webview"))]
        {
            macos::open_privacy_settings()
        }

        #[cfg(all(target_os = "macos", not(feature = "webview")))]
        {
            PrivacySettingsOpenOutcome::Unsupported
        }

        #[cfg(not(target_os = "macos"))]
        {
            PrivacySettingsOpenOutcome::NotApplicable
        }
    }

    fn show_pre_window_error_dialog(&self, title: &str, detail: &str) -> StartupDialogOutcome {
        #[cfg(target_os = "windows")]
        {
            windows::show_pre_window_error_dialog(title, detail)
        }

        #[cfg(all(target_os = "macos", feature = "webview"))]
        {
            macos::show_pre_window_error_dialog(title, detail)
        }

        #[cfg(all(target_os = "linux", feature = "webview"))]
        {
            linux::show_pre_window_error_dialog(title, detail)
        }

        #[cfg(not(any(
            target_os = "windows",
            all(target_os = "macos", feature = "webview"),
            all(target_os = "linux", feature = "webview")
        )))]
        {
            let _ = (title, detail);
            StartupDialogOutcome::Unsupported
        }
    }
}

/// The Windows adapter reads the registered WebView2 runtime version. Registered
/// is not the same as creatable, but a definite absence is direct evidence.
#[cfg(target_os = "windows")]
fn runtime_availability() -> WebViewAvailability {
    webview2::webview_availability()
}

/// WebKitGTK is linked into the executable; its own version report is the evidence.
#[cfg(all(target_os = "linux", feature = "webview"))]
fn runtime_availability() -> WebViewAvailability {
    webview_runtime::linked_runtime_availability(webkit_gtk::linked_version().as_deref())
}

/// The WebKit framework that defines `WKWebView` reports its bundle version.
#[cfg(all(target_os = "macos", feature = "webview"))]
fn runtime_availability() -> WebViewAvailability {
    webview_runtime::linked_runtime_availability(webkit_framework::framework_version().as_deref())
}

/// No adapter observes a runtime here, and a compiled adapter does not prove
/// that a desktop session can create and use a webview.
#[cfg(not(any(
    target_os = "windows",
    all(any(target_os = "linux", target_os = "macos"), feature = "webview")
)))]
fn runtime_availability() -> WebViewAvailability {
    WebViewAvailability::Unknown
}
