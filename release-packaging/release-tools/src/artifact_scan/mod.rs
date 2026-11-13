//! Opened-package scan (F15 release stages 4 and 5, 라이선스 관문 and 고지·SBOM 대조;
//! FR5.HD1.app-host.DD1).
//!
//! The workflow opens each released package into `<extracted>/<package file name>/` with the
//! format's own tool (tar, dpkg-deb, unzip, hdiutil, or the packaged executable's own
//! `notices` command). This module then checks the opened tree: it must contain the repository
//! LICENSE and the generated THIRD-PARTY-NOTICES, byte for byte, where the format registry says
//! they belong, and nothing that must not ship (see [`denied_members`]). The scan fails closed: an
//! unreadable directory, a symbolic link that leaves the tree and a package that was not opened are
//! failures. It also reads the link list of every staged executable ([`crate::link_list`]).

use crate::{
    link_list::check_link_list,
    release_gate::GateFailure,
    release_inventory::{PackageFormat, ReleaseManifest, StagedFileKind},
    sbom::NOTICE_MISMATCH,
};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

mod members;
#[cfg(test)]
mod tests;

pub use members::{denied_members, link_escapes};

pub use crate::release_gate::LICENSE_DENIED;

/// Name of the license document inside every package.
pub const LICENSE_FILE: &str = "LICENSE";
/// Name of the notices document inside every package.
pub const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.txt";

fn mismatch(detail: impl Into<String>) -> GateFailure {
    GateFailure::new(NOTICE_MISMATCH, detail)
}

/// Returns the directory that holds the two documents inside an opened package.
///
/// The locations follow the packagers: a tar.gz has one top-level folder (a real folder, not a
/// link), a deb installs them under `usr/share/doc/diskunfold`, and a portable executable, MSIX or
/// DMG has them at the root of what the workflow opened (the executable's own `notices` output for
/// the portable executable).
fn document_dir(format: PackageFormat, tree: &Path) -> Result<PathBuf, GateFailure> {
    match format {
        PackageFormat::Deb => Ok(tree.join("usr/share/doc/diskunfold")),
        PackageFormat::TarGz => {
            let entries = fs::read_dir(tree)
                .and_then(|entries| entries.collect::<Result<Vec<_>, _>>())
                .map_err(|error| mismatch(format!("{}: {error}", tree.display())))?;
            match entries.as_slice() {
                [only] if only.file_type().is_ok_and(|kind| kind.is_dir()) => Ok(only.path()),
                _ => Err(mismatch(
                    "tar.gz must hold exactly one top-level folder, not a link or a file",
                )),
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
/// file, or differs from the expected bytes; `release.license_denied` for a member that must not
/// ship or a tree that cannot be read.
pub fn scan_package(
    package: &str,
    format: PackageFormat,
    tree: &Path,
    allowed_binary: &str,
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
    let denied = denied_members(tree, allowed_binary).map_err(|failure| {
        GateFailure::new(LICENSE_DENIED, format!("{package}: {}", failure.detail))
    })?;
    if denied.is_empty() {
        Ok(())
    } else {
        Err(GateFailure::new(
            LICENSE_DENIED,
            format!("{package} contains: {}", denied.join(", ")),
        ))
    }
}

fn plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && Path::new(name).components().count() == 1
        && matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
}

fn staged_executable(
    manifest: &ReleaseManifest,
    package: &str,
    staging: &Path,
) -> Result<PathBuf, GateFailure> {
    let entry = manifest
        .staged_files
        .iter()
        .find(|file| file.artifact_file_name == package && file.kind == StagedFileKind::Executable)
        .ok_or_else(|| {
            GateFailure::new(
                LICENSE_DENIED,
                format!("{package}: no staged executable to read the link list from"),
            )
        })?;
    let safe = entry
        .relative_path
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    if !safe {
        return Err(GateFailure::new(
            LICENSE_DENIED,
            format!(
                "{package}: unsafe executable path {}",
                entry.relative_path.display()
            ),
        ));
    }
    Ok(staging.join(&entry.relative_path))
}

/// Checks every package row against its opened tree under `extracted` and the link list of its
/// staged executable under `staging`. Returns the number of packages scanned.
///
/// # Errors
/// Returns the first failure; a package the workflow did not open fails with
/// `release.notice_mismatch`, so a skipped scan cannot pass.
pub fn scan_release(
    manifest: &ReleaseManifest,
    extracted: &Path,
    staging: &Path,
    license: &[u8],
    notices: &[u8],
) -> Result<usize, GateFailure> {
    for row in &manifest.rows {
        let tree = extracted.join(&row.file_name);
        if !plain_file_name(&row.file_name) || !tree.is_dir() {
            return Err(mismatch(format!(
                "{} was not opened for the scan",
                row.file_name
            )));
        }
        scan_package(
            &row.file_name,
            row.format,
            &tree,
            &row.executable_file_name,
            license,
            notices,
        )?;
        let executable = staged_executable(manifest, &row.file_name, staging)?;
        let bytes = fs::read(&executable).map_err(|error| {
            GateFailure::new(
                LICENSE_DENIED,
                format!("{}: {}: {error}", row.file_name, executable.display()),
            )
        })?;
        check_link_list(&row.file_name, &bytes)?;
    }
    Ok(manifest.rows.len())
}
