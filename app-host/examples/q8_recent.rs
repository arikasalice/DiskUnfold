//! Isolated F01 Q8 measurements through the real service and SQLite adapter.
//! Windows only; this executable never opens the application's data directory.

#[cfg(windows)]
mod windows;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This isolated benchmark requires Windows.");
}
