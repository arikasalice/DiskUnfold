//! Select the native webview adapter for the operating system.
mod contract;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod windows;

pub use contract::{Asset, WebApp};
#[cfg(target_os = "windows")]
pub use contract::{
    DispatchLane, ResponseDelivery, ResponseDeliveryCallback, ResponseDeliveryObserver,
};
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub use contract::{EventCallbacks, EventPostFailure, Events};

/// Runs the Linux webview and delivers events to its trusted top-level page.
#[cfg(target_os = "linux")]
pub fn run_with_events(app: WebApp, events: Events) -> Result<(), Box<dyn std::error::Error>> {
    linux::run(app, Some(events))
}

/// Runs the Windows webview and delivers events to its trusted top-level page.
#[cfg(target_os = "windows")]
pub fn run_with_events(app: WebApp, events: Events) -> Result<(), Box<dyn std::error::Error>> {
    windows::run(app, Some(events))
}

pub fn run(app: WebApp) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    return linux::run(app, None);
    #[cfg(target_os = "macos")]
    return macos::run(app);
    #[cfg(target_os = "windows")]
    return windows::run(app, None);
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        Err("Unsupported desktop target".into())
    }
}
