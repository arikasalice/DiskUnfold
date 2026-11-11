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
    resource_digests: BTreeMap<String, [u8; 32]>,
    configured_languages: Vec<String>,
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
        let mut resource_digests = BTreeMap::new();
        for resource in checked {
            let bundle = parse(resource.bytes)?;
            if bundle.language != resource.language || bundle.version != resource.version {
                return Err(RegistrationError);
            }
            unique(bundle.messages.iter().map(|(key, _)| key.as_str()))?;
            resource_digests.insert(bundle.language.clone(), resource.expected_hash);
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
            resource_digests,
            configured_languages: resources
                .iter()
                .map(|row| row.language.to_owned())
                .collect(),
            integrity_warnings: warnings,
        })
    }

    /// Digest of exactly the bytes that passed registration, never a reserialization.
    pub fn resource_digest(&self, language: &str) -> Option<[u8; 32]> {
        self.resource_digests.get(language).copied()
    }

    /// Self-name key only for an integrity-verified, registered bundle containing it.
    pub fn self_name_key(&self, language: &str) -> Option<&'static str> {
        let key = match language {
            "en" => "locale.language.en",
            "ko" => "locale.language.ko",
            _ => return None,
        };
        let bundle = self.bundle(language)?;
        bundle
            .messages
            .iter()
            .any(|(candidate, message)| candidate == key && message.parameters.is_empty())
            .then_some(key)
    }

    /// Calculate revisions from registered source bytes and exact metadata bytes.
    /// The caller must admit and validate a real metadata table before using this
    /// value in a bundle; the registry does not provide metadata or a wire body.
    pub fn revisions(&self, language: &str, metadata_bytes: &[u8]) -> Option<BundleRevisions> {
        let resource = self.resource_digest(language)?;
        let metadata = blake3::hash(metadata_bytes);
        Some(BundleRevisions {
            metadata_revision: metadata.to_hex().to_string(),
            bundle_revision: bundle_revision(language, &resource, metadata.as_bytes())?,
        })
    }

    pub fn bundle(&self, language: &str) -> Option<&Bundle> {
        self.bundles.get(language)
    }

    pub fn languages(&self) -> impl Iterator<Item = &str> {
        self.bundles.keys().map(String::as_str)
    }

    /// Returns the build inventory, including resources rejected for integrity.
    pub fn configured_languages(&self) -> impl Iterator<Item = &str> {
        self.configured_languages.iter().map(String::as_str)
    }

    pub fn integrity_warnings(&self) -> &[String] {
        &self.integrity_warnings
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleRevisions {
    pub metadata_revision: String,
    pub bundle_revision: String,
}

fn bundle_revision(language: &str, resource: &[u8; 32], metadata: &[u8; 32]) -> Option<String> {
    let length = u32::try_from(language.len()).ok()?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"diskunfold.locale.bundle.v1\0");
    hasher.update(&length.to_le_bytes());
    hasher.update(language.as_bytes());
    hasher.update(resource);
    hasher.update(metadata);
    Some(hasher.finalize().to_hex().to_string())
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
