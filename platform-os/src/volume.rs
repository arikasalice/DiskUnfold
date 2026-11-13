//! Read-only OS volume adapters.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::LinuxVolumes;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsVolumes;
