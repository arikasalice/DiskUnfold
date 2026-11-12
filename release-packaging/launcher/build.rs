use std::{env, fs, io, path::PathBuf, process::Command};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../LICENSE");
    println!("cargo:rerun-if-env-changed=RC");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or(io::Error::other("missing OUT_DIR"))?);
    let product_info_path = out_dir.join("product-info.json");
    let product_info = generate_developer_product_info()?;
    fs::write(
        &product_info_path,
        serde_json::to_vec_pretty(&product_info).map_err(io::Error::other)?,
    )?;
    println!(
        "cargo:rustc-env=DU_PRODUCT_INFO_PATH={}",
        product_info_path.display()
    );

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        compile_windows_resource(&out_dir)?;
    }
    Ok(())
}

fn generate_developer_product_info() -> io::Result<serde_json::Value> {
    let repository_root = PathBuf::from("../..");
    let license = fs::read_to_string(repository_root.join("LICENSE"))?;
    let target_os = env::var("CARGO_CFG_TARGET_OS").ok();
    let target_cpu = env::var("CARGO_CFG_TARGET_ARCH").ok();

    Ok(serde_json::json!({
        "build": {
            "productVersion": env::var("CARGO_PKG_VERSION").ok(),
            "buildKind": "developer",
            "commit": null,
            "builtAtUtc": null,
            "targetOs": target_os,
            "targetCpu": target_cpu,
            "appDbFormat": null,
            "snapshotFormat": null,
            "supportMatrixRevision": null
        },
        "notices": {
            "license": license,
            "thirdParty": null,
            "sourceCommit": null
        },
        "supportMatrix": null
    }))
}

fn compile_windows_resource(out_dir: &std::path::Path) -> io::Result<()> {
    println!("cargo:rerun-if-changed=windows.rc");
    println!("cargo:rerun-if-changed=../../app-host/icons/icon.ico");
    let out = out_dir.join("diskunfold.res");
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
