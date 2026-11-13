//! Shipped crates from cargo metadata.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde_json::Value;

use super::{Ecosystem, NoticePackage, read_license_files};

/// Collects the registry crates linked into the root package through normal
/// dependency edges on any target. Build and dev edges are not shipped.
///
/// # Errors
/// Returns a message when the metadata lacks the root package or resolve graph.
pub fn cargo_packages(metadata: &Value, root_name: &str) -> Result<Vec<NoticePackage>, String> {
    let packages: BTreeMap<&str, &Value> = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?
        .iter()
        .filter_map(|p| Some((p["id"].as_str()?, p)))
        .collect();
    let nodes: BTreeMap<&str, &Value> = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("cargo metadata has no resolve graph")?
        .iter()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();
    let root = packages
        .iter()
        .find(|(_, p)| p["name"] == root_name)
        .map(|(id, _)| *id)
        .ok_or_else(|| format!("root package {root_name} not found"))?;

    let mut seen = BTreeSet::from([root]);
    let mut queue = vec![root];
    while let Some(id) = queue.pop() {
        let Some(node) = nodes.get(id) else { continue };
        for dep in node["deps"].as_array().into_iter().flatten() {
            let normal = dep["dep_kinds"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|k| k["kind"].is_null()));
            if let Some(dep_id) = dep["pkg"].as_str()
                && normal
                && seen.insert(dep_id)
            {
                queue.push(dep_id);
            }
        }
    }
    let mut result = Vec::new();
    for id in seen {
        let Some(package) = packages.get(id) else {
            continue;
        };
        // Workspace members and path crates are first-party and covered by LICENSE.
        if package["source"].is_null() {
            continue;
        }
        let dir = package["manifest_path"]
            .as_str()
            .and_then(|p| Path::new(p).parent())
            .ok_or("package without manifest path")?;
        result.push(NoticePackage {
            ecosystem: Ecosystem::Cargo,
            name: package["name"].as_str().unwrap_or_default().to_owned(),
            version: package["version"].as_str().unwrap_or_default().to_owned(),
            license: package["license"].as_str().map(str::to_owned),
            texts: read_license_files(dir, package["license_file"].as_str()),
        });
    }
    Ok(result)
}
