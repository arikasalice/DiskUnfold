use std::{env, fs, io, path::PathBuf, process::Command};

// The assembly logic is shared with du-release-tools, which unit-tests it.
mod build_info;

const WATCHED_ENV: [&str; 8] = [
    "DU_BUILD_KIND",
    "DU_SOURCE_COMMIT",
    "DU_RELEASE_TAG",
    "DU_BUILT_AT_UTC",
    "DU_APP_DB_FORMAT",
    "DU_SNAPSHOT_FORMAT",
    "DU_RELEASE_RESOURCES",
    "RC",
];

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_info.rs");
    println!("cargo:rerun-if-changed=../../LICENSE");
    for name in WATCHED_ENV {
        println!("cargo:rerun-if-env-changed={name}");
    }
    watch_git_state();

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or(io::Error::other("missing OUT_DIR"))?);
    let product_info_path = out_dir.join("product-info.json");
    let product_info = generate_product_info()?;
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

fn env_value(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Reruns the script when the checked-out commit or the index changes.
fn watch_git_state() {
    for name in ["HEAD", "index", "logs/HEAD"] {
        if let Some(path) = git_output(&["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn read_resources() -> io::Result<Option<serde_json::Value>> {
    let Some(path) = env_value("DU_RELEASE_RESOURCES") else {
        return Ok(None);
    };
    // Relative paths are resolved from the repository root.
    let path = PathBuf::from("../..").join(path);
    println!("cargo:rerun-if-changed={}", path.display());
    let text = fs::read_to_string(&path)
        .map_err(|error| io::Error::other(format!("read {}: {error}", path.display())))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| io::Error::other(format!("{}: {error}", path.display())))
}

fn generate_product_info() -> io::Result<serde_json::Value> {
    let repository_root = PathBuf::from("../..");
    let detected_commit = git_output(&["rev-parse", "HEAD"]);
    // An unreadable status counts as dirty so a developer build never claims a clean tree.
    let detected_dirty = git_output(&["status", "--porcelain", "--untracked-files=no"])
        .is_none_or(|status| !status.is_empty());
    let inputs = build_info::BuildInputs {
        version: env_value("CARGO_PKG_VERSION"),
        target_os: env_value("CARGO_CFG_TARGET_OS"),
        target_cpu: env_value("CARGO_CFG_TARGET_ARCH"),
        requested_kind: env_value("DU_BUILD_KIND"),
        supplied_commit: env_value("DU_SOURCE_COMMIT"),
        detected_commit,
        detected_dirty,
        release_tag: env_value("DU_RELEASE_TAG"),
        built_at_utc: env_value("DU_BUILT_AT_UTC"),
        app_db_format: env_value("DU_APP_DB_FORMAT"),
        snapshot_format: env_value("DU_SNAPSHOT_FORMAT"),
        license: fs::read_to_string(repository_root.join("LICENSE"))?,
        resources: read_resources()?,
    };
    build_info::assemble(&inputs).map_err(io::Error::other)
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
