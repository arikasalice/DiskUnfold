use std::{env, io, path::PathBuf, process::Command};

fn main() -> io::Result<()> {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }

    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-changed=windows.rc");
    println!("cargo:rerun-if-changed=../../app-host/icons/icon.ico");
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or(io::Error::other("missing OUT_DIR"))?)
        .join("diskunfold.res");
    let mut compiler = env::var_os("RC").map_or_else(resource_compiler, Command::new);
    let status = compiler
        .arg("/nologo")
        .arg("/fo")
        .arg(&out)
        .arg("windows.rc")
        .status()
        .map_err(|error| io::Error::other(format!("Start Windows resource compiler: {error}")))?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "Windows resource compiler failed: {status}"
        )));
    }
    println!("cargo:rustc-link-arg-bin=diskunfold={}", out.display());
    Ok(())
}

#[cfg(windows)]
fn resource_compiler() -> Command {
    let target = env::var("TARGET").unwrap_or_default();
    find_msvc_tools::find(&target, "rc.exe").unwrap_or_else(|| Command::new("rc.exe"))
}

#[cfg(not(windows))]
fn resource_compiler() -> Command {
    Command::new("llvm-rc")
}
