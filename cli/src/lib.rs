//! Parse CLI arguments and present engine results.

use std::{ffi::OsStr, process::ExitCode};

pub fn run(arg: &OsStr, extra_args: bool) -> ExitCode {
    #[cfg(all(windows, feature = "windows-console"))]
    du_platform_os::windows_console::attach_parent();

    match (arg.to_str(), extra_args) {
        (Some("--help" | "-h"), false) => {
            println!("DiskUnfold\n\nUsage: diskunfold [--help | --version]");
        }
        (Some("--version" | "-V"), false) => {
            println!("DiskUnfold {}", env!("CARGO_PKG_VERSION"));
        }
        _ => {
            eprintln!("DiskUnfold: unsupported arguments. See --help.");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
