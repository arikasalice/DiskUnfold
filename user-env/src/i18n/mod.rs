//! Immutable, startup-only language bundle registration.

use std::collections::{BTreeMap, BTreeSet};

pub mod embedded;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub parameters: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bundle {
    pub language: String,
    pub version: String,
    pub messages: Vec<(String, Message)>,
    pub format_table: serde_json::Value,
    pub plural_rule: String,
    pub fallback_language: String,
}

/// A composition root supplies the exact embedded bytes and their build-time digest.
pub struct BundleResource<'a> {
    pub language: &'a str,
    pub version: &'a str,
    pub bytes: &'a [u8],
    pub expected_hash: [u8; 32],
}

#[derive(Debug, PartialEq, Eq)]
pub struct RegistrationError;

impl RegistrationError {
    pub const fn reason_key(&self) -> &'static str {
        "locale.registration_failed"
    }
}

#[derive(Debug)]
pub struct Registry {
    bundles: BTreeMap<String, Bundle>,
    integrity_warnings: Vec<String>,
}

impl Registry {
    /// The parser must derive the bundle exclusively from the supplied resource bytes.
    pub fn register(
        resources: &[BundleResource<'_>],
        declared_keys: &[String],
        metadata_keys: &[String],
        parse: impl Fn(&[u8]) -> Result<Bundle, RegistrationError>,
    ) -> Result<Self, RegistrationError> {
        let mut checked = Vec::new();
        let mut warnings = Vec::new();
        for resource in resources {
            if blake3::hash(resource.bytes).as_bytes() != &resource.expected_hash {
                if resource.language == "ko" {
                    warnings.push("i18n.bundle_integrity_failed".to_owned());
                    continue;
                }
                return Err(RegistrationError);
            }
            checked.push(resource);
        }
        // Count registration rows, including rejected Korean resources.
        unique(resources.iter().map(|resource| resource.language))?;
        let mut bundles = BTreeMap::new();
        for resource in checked {
            let bundle = parse(resource.bytes)?;
            if bundle.language != resource.language || bundle.version != resource.version {
                return Err(RegistrationError);
            }
            unique(bundle.messages.iter().map(|(key, _)| key.as_str()))?;
            bundles.insert(bundle.language.clone(), bundle);
        }
        unique(declared_keys.iter().map(String::as_str))?;
        let english = bundles.get("en").ok_or(RegistrationError)?;
        let english_keys: BTreeSet<_> = english.messages.iter().map(|(key, _)| key).collect();
        if declared_keys.iter().any(|key| !english_keys.contains(key)) {
            return Err(RegistrationError);
        }
        unique(metadata_keys.iter().map(String::as_str))?;
        Ok(Self {
            bundles,
            integrity_warnings: warnings,
        })
    }

    pub fn bundle(&self, language: &str) -> Option<&Bundle> {
        self.bundles.get(language)
    }

    pub fn languages(&self) -> impl Iterator<Item = &str> {
        self.bundles.keys().map(String::as_str)
    }

    pub fn integrity_warnings(&self) -> &[String] {
        &self.integrity_warnings
    }
}

fn unique<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<(), RegistrationError> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(RegistrationError);
        }
    }
    Ok(())
}

/// Check declared parameter names and types against each registered message.
/// Missing declared keys and mismatches are returned; extra keys are left to the merge gate.
pub fn parameter_mismatches(
    registry: &Registry,
    declarations: &BTreeMap<String, BTreeMap<String, String>>,
) -> Vec<(String, String)> {
    let mut failures = Vec::new();
    for (language, bundle) in &registry.bundles {
        let messages: BTreeMap<_, _> = bundle.messages.iter().map(|(k, v)| (k, v)).collect();
        for (key, parameters) in declarations {
            if messages
                .get(key)
                .is_none_or(|message| &message.parameters != parameters)
            {
                failures.push((language.clone(), key.clone()));
            }
        }
    }
    failures
}

#[cfg(test)]
mod tests;
