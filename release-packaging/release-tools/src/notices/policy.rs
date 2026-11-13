//! License allow list and banned crates from deny.toml.

use std::collections::BTreeSet;

use super::{Ecosystem, LICENSE_DENIED, LICENSE_UNKNOWN, NoticePackage, PACKAGE_BANNED};
use crate::license_policy::permits;

/// License policy read from deny.toml.
#[derive(Clone, Debug, Default)]
pub struct DenyPolicy {
    /// Allowed SPDX identifiers.
    pub allowed: BTreeSet<String>,
    /// Crates that must never ship.
    pub banned_crates: BTreeSet<String>,
}

impl DenyPolicy {
    /// Parses the licenses.allow and bans.deny tables.
    ///
    /// # Errors
    /// Returns a message when the TOML is invalid or has no allow list.
    pub fn parse(text: &str) -> Result<Self, String> {
        let table: toml::Table = text.parse().map_err(|e| format!("deny.toml: {e}"))?;
        let allowed: BTreeSet<String> = table
            .get("licenses")
            .and_then(|v| v.get("allow"))
            .and_then(toml::Value::as_array)
            .ok_or("deny.toml has no licenses.allow list")?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        if allowed.is_empty() {
            return Err("deny.toml licenses.allow is empty".to_owned());
        }
        let banned_crates = table
            .get("bans")
            .and_then(|v| v.get("deny"))
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.get("name")?.as_str().map(str::to_owned))
            .collect();
        Ok(Self {
            allowed,
            banned_crates,
        })
    }
}

/// Older crates write OR as a slash (MIT/Apache-2.0); cargo reads that form as OR.
fn cargo_spelling(package: &NoticePackage, expression: &str) -> String {
    if package.ecosystem == Ecosystem::Cargo {
        expression.replace('/', " OR ")
    } else {
        expression.to_owned()
    }
}

/// One policy finding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Violation {
    /// Stable release reason key.
    pub reason_key: &'static str,
    /// Package label, ecosystem:name@version.
    pub package: String,
    /// License expression involved, if any.
    pub license: Option<String>,
}

/// Checks every package against the policy; findings are sorted.
pub fn check_policy(packages: &[NoticePackage], policy: &DenyPolicy) -> Vec<Violation> {
    let mut found = Vec::new();
    for package in packages {
        let label = format!(
            "{}:{}@{}",
            package.ecosystem.label(),
            package.name,
            package.version
        );
        let mut add = |reason_key| {
            found.push(Violation {
                reason_key,
                package: label.clone(),
                license: package.license.clone(),
            });
        };
        if package.ecosystem == Ecosystem::Cargo && policy.banned_crates.contains(&package.name) {
            add(PACKAGE_BANNED);
        }
        match package.license.as_deref() {
            None => add(LICENSE_UNKNOWN),
            Some(expression) if !permits(&cargo_spelling(package, expression), &policy.allowed) => {
                add(LICENSE_DENIED);
            }
            Some(_) => {}
        }
    }
    found.sort_by(|a, b| (a.reason_key, &a.package).cmp(&(b.reason_key, &b.package)));
    found
}
