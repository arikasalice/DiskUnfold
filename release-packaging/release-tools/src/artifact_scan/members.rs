//! Member checks on an opened package tree.

use std::{fs, path::Path};

fn denied_reason(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("libdbus") {
        Some("libdbus")
    } else if lower.contains("lgpl")
        || lower.contains("agpl")
        || lower.starts_with("gpl")
        || lower.contains("-gpl")
        || lower.contains(".gpl")
        || lower.starts_with("copying.lesser")
    {
        Some("a GPL-family license file")
    } else if lower.ends_with(".so") || lower.contains(".so.") {
        Some("a bundled shared library")
    } else {
        None
    }
}

fn walk(dir: &Path, prefix: &str, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{prefix}{name}");
        // Symbolic links are never followed, so a link out of the tree is not scanned.
        let is_dir = fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.is_dir());
        if is_dir {
            walk(&entry.path(), &format!("{relative}/"), found);
        } else if let Some(reason) = denied_reason(&name) {
            found.push(format!("{relative} ({reason})"));
        }
    }
}

/// Lists the members of the tree whose names show a shared library, libdbus or a GPL-family
/// license file. Linux packages must link system libraries only and the product ships no
/// copyleft code (DEC12).
pub fn copyleft_members(tree: &Path) -> Vec<String> {
    let mut found = Vec::new();
    walk(tree, "", &mut found);
    found
}
