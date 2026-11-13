//! THIRD-PARTY-NOTICES generation from Cargo metadata and the npm lockfile
//! (F15 FR5.HD1, FR5.HD1.app-host.DD1).

mod cargo;
mod npm;
mod policy;
mod render;
#[cfg(test)]
mod tests;

use std::{fs, path::Path, process::Command};

pub use cargo::cargo_packages;
pub use npm::npm_packages;
pub use policy::{DenyPolicy, Violation, check_policy};
pub use render::render_notices;

/// Reason key: a shipped package has no license expression.
pub const LICENSE_UNKNOWN: &str = "release.notices_license_unknown";
/// Reason key: a shipped package license is outside the allow list.
pub const LICENSE_DENIED: &str = "release.notices_license_denied";
/// Reason key: a shipped package is on the banned crate list.
pub const PACKAGE_BANNED: &str = "release.notices_package_banned";

/// Package ecosystem of a shipped component.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Ecosystem {
    /// A crates.io crate linked into the executable.
    Cargo,
    /// An npm package bundled into the embedded web assets.
    Npm,
}

impl Ecosystem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Npm => "npm",
        }
    }
}

/// One license file found next to a package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LicenseText {
    /// File name inside the package.
    pub file: String,
    /// File content.
    pub text: String,
}

/// One shipped third-party package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoticePackage {
    /// Package ecosystem.
    pub ecosystem: Ecosystem,
    /// Package name.
    pub name: String,
    /// Locked version.
    pub version: String,
    /// SPDX license expression from the package metadata.
    pub license: Option<String>,
    /// License texts found in the package.
    pub texts: Vec<LicenseText>,
    /// Authors recorded in the package manifest, when present.
    pub authors: Vec<String>,
}

/// Generated notices and the policy findings for them.
#[derive(Clone, Debug)]
pub struct NoticeReport {
    /// THIRD-PARTY-NOTICES content.
    pub text: String,
    /// Number of listed packages.
    pub package_count: usize,
    /// Policy findings; the release gate fails when this is not empty.
    pub violations: Vec<Violation>,
}

/// Reads license files whose names start with a common license stem.
pub(crate) fn read_license_files(dir: &Path, extra: Option<&str>) -> Vec<LicenseText> {
    const STEMS: [&str; 5] = ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE"];
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut texts: Vec<LicenseText> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let upper = file.to_ascii_uppercase();
            let wanted = STEMS.iter().any(|stem| upper.starts_with(stem))
                || extra.is_some_and(|name| name == file);
            let bytes = if wanted {
                fs::read(entry.path()).ok()?
            } else {
                return None;
            };
            Some(LicenseText {
                file,
                text: String::from_utf8_lossy(&bytes).into_owned(),
            })
        })
        .collect();
    texts.sort_by(|a, b| a.file.cmp(&b.file));
    texts
}

/// Collects the third-party packages that ship in the launcher and its embedded
/// web assets, sorted by ecosystem, name and version.
///
/// Runs cargo metadata with the locked lockfile and reads gui-web/package-lock.json.
/// npm license texts come from an installed gui-web/node_modules tree when present.
///
/// # Errors
/// Returns a message when cargo metadata or the lockfile cannot be read.
pub fn shipped_packages(repo: &Path) -> Result<Vec<NoticePackage>, String> {
    shipped_packages_for(repo, None)
}

/// Like [`shipped_packages`], limited to the Cargo dependencies that a build for the Rust target
/// triple links (`cargo metadata --filter-platform`). The npm packages are the same for every
/// target. `None` lists the dependencies of every target, which is what THIRD-PARTY-NOTICES covers.
///
/// # Errors
/// Returns a message when cargo metadata or the lockfile cannot be read.
pub fn shipped_packages_for(
    repo: &Path,
    triple: Option<&str>,
) -> Result<Vec<NoticePackage>, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .current_dir(repo)
        .args(["metadata", "--locked", "--format-version", "1"]);
    if let Some(triple) = triple {
        command.args(["--filter-platform", triple]);
    }
    let output = command
        .output()
        .map_err(|e| format!("start cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata --locked failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let metadata =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let lock_text = fs::read_to_string(repo.join("gui-web/package-lock.json"))
        .map_err(|e| format!("read gui-web/package-lock.json: {e}"))?;
    let lock = serde_json::from_str(&lock_text).map_err(|e| format!("package-lock.json: {e}"))?;
    let mut packages = cargo_packages(&metadata, "du-launcher")?;
    packages.extend(npm_packages(&lock, &repo.join("gui-web"))?);
    packages.sort_by(|a, b| {
        (a.ecosystem, &a.name, &a.version).cmp(&(b.ecosystem, &b.name, &b.version))
    });
    Ok(packages)
}

/// Generates notices for the launcher from a repository checkout.
///
/// Reads the packages from [`shipped_packages`] and the license policy in
/// deny.toml. The SPDX expression stands in for a license text that a package
/// does not bundle.
///
/// # Errors
/// Returns a message when cargo metadata, the lockfile or the policy cannot be read.
pub fn generate(repo: &Path) -> Result<NoticeReport, String> {
    let deny =
        fs::read_to_string(repo.join("deny.toml")).map_err(|e| format!("read deny.toml: {e}"))?;
    let policy = DenyPolicy::parse(&deny)?;
    let packages = shipped_packages(repo)?;
    let violations = check_policy(&packages, &policy);
    Ok(NoticeReport {
        text: render_notices(&packages),
        package_count: packages.len(),
        violations,
    })
}
