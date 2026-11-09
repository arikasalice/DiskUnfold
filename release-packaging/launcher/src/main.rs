#![cfg_attr(all(windows, feature = "gui"), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    match args.next() {
        Some(arg) => du_cli::run(std::iter::once(arg).chain(args)),
        None => launch_gui(),
    }
}

#[cfg(feature = "gui")]
fn launch_gui() -> ExitCode {
    match du_app_host::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("DiskUnfold: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "gui"))]
fn launch_gui() -> ExitCode {
    eprintln!("DiskUnfold: this development build has no GUI");
    ExitCode::from(2)
}
