//! Opened-package scan (F15 release stages 4 and 5, 라이선스 관문 and 고지·SBOM 대조;
//! FR5.HD1.app-host.DD1).
//!
//! The workflow opens each released package into `<extracted>/<package file name>/` with the
//! format's own tool (tar, dpkg-deb, unzip, hdiutil, or the packaged executable's own
//! `notices` command). This module then checks the opened tree: it must contain the repository
//! LICENSE and the generated THIRD-PARTY-NOTICES, byte for byte, where the format registry says
//! they belong, and no shared library or GPL-family license file.

use crate::{
    release_gate::GateFailure,
    release_inventory::{PackageFormat, ReleaseManifest},
    sbom::NOTICE_MISMATCH,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

mod members;
#[cfg(test)]
mod tests;

pub use members::copyleft_members;

/// Reason key: a package holds a shared library or a GPL-family license file.
pub const LICENSE_DENIED: &str = "release.license_denied";

/// Name of the license document inside every package.
pub const LICENSE_FILE: &str = "LICENSE";
/// Name of the notices document inside every package.
pub const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.txt";

fn mismatch(detail: impl Into<String>) -> GateFailure {
    GateFailure::new(NOTICE_MISMATCH, detail)
}

/// Returns the directory that holds the two documents inside an opened package.
///
/// The locations follow the packagers: a tar.gz has one top-level folder, a deb installs them
/// under `usr/share/doc/diskunfold`, and a portable executable, MSIX or DMG has them at the root
/// of what the workflow opened (the executable's own `notices` output for the portable
/// executable).
fn document_dir(format: PackageFormat, tree: &Path) -> Result<PathBuf, GateFailure> {
    match format {
        PackageFormat::Deb => Ok(tree.join("usr/share/doc/diskunfold")),
        PackageFormat::TarGz => {
            let entries: Vec<_> = fs::read_dir(tree)
                .map_err(|error| mismatch(format!("{}: {error}", tree.display())))?
                .filter_map(Result::ok)
                .collect();
            match entries.as_slice() {
                [only] if only.path().is_dir() => Ok(only.path()),
                _ => Err(mismatch("tar.gz must hold exactly one top-level folder")),
            }
        }
        PackageFormat::PortableExe | PackageFormat::Msix | PackageFormat::Dmg => {
            Ok(tree.to_path_buf())
        }
        PackageFormat::Msi => Err(mismatch("no scan layout is registered for msi packages")),
    }
}

fn read_document(dir: &Path, name: &str, package: &str) -> Result<Vec<u8>, GateFailure> {
    let path = dir.join(name);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| mismatch(format!("{package} does not ship {name}")))?;
    if !metadata.is_file() {
        return Err(mismatch(format!("{package}: {name} is not a regular file")));
    }
    fs::read(&path).map_err(|error| mismatch(format!("{package}: {name}: {error}")))
}

/// Checks one opened package.
///
/// # Errors
/// `release.notice_mismatch` when LICENSE or THIRD-PARTY-NOTICES is missing, is not a regular
/// file, or differs from the expected bytes; `release.license_denied` for a shared library or a
/// GPL-family license file among its members.
pub fn scan_package(
    package: &str,
    format: PackageFormat,
    tree: &Path,
    license: &[u8],
    notices: &[u8],
) -> Result<(), GateFailure> {
    let dir = document_dir(format, tree)?;
    for (name, expected) in [(LICENSE_FILE, license), (NOTICES_FILE, notices)] {
        if read_document(&dir, name, package)? != expected {
            return Err(mismatch(format!(
                "{package}: {name} differs from the generated document"
            )));
        }
    }
    let denied = copyleft_members(tree);
    if denied.is_empty() {
        Ok(())
    } else {
        Err(GateFailure::new(
            LICENSE_DENIED,
            format!("{package} contains: {}", denied.join(", ")),
        ))
    }
}

/// Checks every package row against its opened tree under `extracted`. Returns the number of
/// packages scanned.
///
/// # Errors
/// Returns the first failure; a package the workflow did not open fails with
/// `release.notice_mismatch`, so a skipped scan cannot pass.
pub fn scan_release(
    manifest: &ReleaseManifest,
    extracted: &Path,
    license: &[u8],
    notices: &[u8],
) -> Result<usize, GateFailure> {
    for row in &manifest.rows {
        let safe = !row.file_name.contains(['/', '\\']) && row.file_name != "..";
        let tree = extracted.join(&row.file_name);
        if !safe || !tree.is_dir() {
            return Err(mismatch(format!(
                "{} was not opened for the scan",
                row.file_name
            )));
        }
        scan_package(&row.file_name, row.format, &tree, license, notices)?;
    }
    Ok(manifest.rows.len())
}
