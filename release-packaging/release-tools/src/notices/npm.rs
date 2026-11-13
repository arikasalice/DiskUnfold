//! Shipped npm packages from the lockfile.

use std::path::Path;

use serde_json::Value;

use super::{Ecosystem, NoticePackage, read_license_files};

/// Collects production packages recorded in package-lock.json. License texts
/// come from the installed tree under web_root when it exists.
///
/// # Errors
/// Returns a message when the lockfile has no packages table.
pub fn npm_packages(lock: &Value, web_root: &Path) -> Result<Vec<NoticePackage>, String> {
    let table = lock["packages"]
        .as_object()
        .ok_or("package-lock.json has no packages table")?;
    let mut result = Vec::new();
    for (path, package) in table {
        if path.is_empty() || package["dev"] == true {
            continue;
        }
        let Some(version) = package["version"].as_str() else {
            continue;
        };
        let name = package["name"]
            .as_str()
            .or_else(|| path.rsplit("node_modules/").next())
            .unwrap_or(path);
        result.push(NoticePackage {
            ecosystem: Ecosystem::Npm,
            name: name.to_owned(),
            version: version.to_owned(),
            license: package["license"].as_str().map(str::to_owned),
            texts: read_license_files(&web_root.join(path), None),
        });
    }
    Ok(result)
}
