//! Member checks on an opened package tree. They fail closed: an unreadable directory, a link out
//! of the tree and a bundled binary are failures, not omissions.

use crate::{
    link_list::detect,
    release_gate::{GateFailure, LICENSE_DENIED},
};
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path},
};

fn denied(detail: String) -> GateFailure {
    GateFailure::new(LICENSE_DENIED, detail)
}

fn denied_name_reason(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("libdbus") {
        Some("libdbus")
    } else if lower.contains("lgpl")
        || lower.contains("agpl")
        || lower.starts_with("gpl")
        || lower.contains("-gpl")
        || lower.contains(".gpl")
        || lower == "copying"
        || lower.starts_with("copying.")
    {
        Some("a GPL-family license file")
    } else if lower.ends_with(".so") || lower.contains(".so.") {
        Some("a bundled shared library")
    } else {
        None
    }
}

/// Whether a symbolic link at `directory` (components below the tree root) pointing at `target`
/// leaves the tree. An absolute target always does.
pub fn link_escapes(directory: &[String], target: &Path) -> bool {
    let mut depth = directory.len();
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir => {
                if depth == 0 {
                    return true;
                }
                depth -= 1;
            }
            Component::RootDir | Component::Prefix(_) => return true,
        }
    }
    false
}

fn starts_with_binary_magic(path: &Path) -> Result<bool, std::io::Error> {
    let mut magic = [0_u8; 4];
    let read = File::open(path)?.read(&mut magic)?;
    Ok(detect(&magic[..read]).is_some())
}

fn walk(
    dir: &Path,
    below_root: &mut Vec<String>,
    allowed_binary: &str,
    found: &mut Vec<String>,
) -> Result<(), GateFailure> {
    let unreadable = |path: &Path, error: std::io::Error| {
        denied(format!("{} cannot be read: {error}", path.display()))
    };
    let mut entries = fs::read_dir(dir)
        .map_err(|error| unreadable(dir, error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| unreadable(dir, error))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = [below_root.as_slice(), std::slice::from_ref(&name)]
            .concat()
            .join("/");
        // The file type is the entry's own, so a symbolic link is never followed.
        let kind = entry
            .file_type()
            .map_err(|error| unreadable(&path, error))?;
        if kind.is_symlink() {
            let target = fs::read_link(&path).map_err(|error| unreadable(&path, error))?;
            if link_escapes(below_root, &target) {
                found.push(format!(
                    "{relative} (a link to {} that leaves the package)",
                    target.display()
                ));
            }
        } else if kind.is_dir() {
            below_root.push(name);
            walk(&path, below_root, allowed_binary, found)?;
            below_root.pop();
        } else if let Some(reason) = denied_name_reason(&name) {
            found.push(format!("{relative} ({reason})"));
        } else if name != allowed_binary
            && starts_with_binary_magic(&path).map_err(|error| unreadable(&path, error))?
        {
            found.push(format!(
                "{relative} (a bundled executable or library; only {allowed_binary} may ship)"
            ));
        }
    }
    Ok(())
}

/// Lists the members of the tree that must not ship: shared libraries, libdbus, GPL-family license
/// files, binaries other than `allowed_binary` (recognized by their magic bytes, whatever the file
/// is called), and symbolic links that leave the tree.
///
/// # Errors
/// Returns `release.license_denied` when a directory or file in the tree cannot be read.
pub fn denied_members(tree: &Path, allowed_binary: &str) -> Result<Vec<String>, GateFailure> {
    let mut found = Vec::new();
    walk(tree, &mut Vec::new(), allowed_binary, &mut found)?;
    Ok(found)
}
