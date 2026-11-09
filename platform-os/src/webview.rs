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

pub fn run(app: WebApp) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    return linux::run(app);
    #[cfg(target_os = "macos")]
    return macos::run(app);
    #[cfg(target_os = "windows")]
    return windows::run(app);
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        Err("Unsupported desktop target".into())
    }
}
