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
            // A compiled adapter or installed library does not prove that a
            // desktop session can create and use its webview successfully.
            WebViewAvailability::Unknown
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
