//! Immutable, startup-only language bundle registration.

use std::collections::{BTreeMap, BTreeSet};

use du_core_contracts::locale_bundle::{
    LocaleBundleBody, LocaleBundleRequest, LocaleBundleResponse, ReasonMessageMeta,
};

mod bundle_body;
mod declared;
pub mod embedded;
mod metadata;
mod missing;
mod resolver;

pub use missing::{MISSING_MESSAGE_CAPACITY, MissingMessageRecord};
pub use resolver::{ResolvedMessage, ResolvedReason, UNKNOWN_REASON_MESSAGE_KEY};

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

/// The exact embedded bytes and build-time digest for the language-neutral reason table.
pub struct ReasonMetadataResource<'a> {
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
    reason_messages: BTreeMap<String, ReasonMessageMeta>,
    metadata_revision: String,
    bundle_revisions: BTreeMap<String, String>,
    bundle_bodies: BTreeMap<String, LocaleBundleBody>,
    configured_languages: Vec<String>,
    integrity_warnings: Vec<String>,
    missing_messages: missing::MissingMessageQueue,
}

impl Registry {
    /// The parser must derive the bundle exclusively from the supplied resource bytes.
    pub fn register(
        resources: &[BundleResource<'_>],
        declared_keys: &[String],
        metadata_keys: &[String],
        parse: impl Fn(&[u8]) -> Result<Bundle, RegistrationError>,
    ) -> Result<Self, RegistrationError> {
        let metadata_bytes = b"[]";
        Self::register_internal(
            resources,
            declared_keys,
            metadata_keys,
            metadata_bytes,
            *blake3::hash(metadata_bytes).as_bytes(),
            false,
            parse,
        )
    }

    /// Registers resources against the exact embedded reason metadata bytes.
    pub fn register_with_reason_metadata(
        resources: &[BundleResource<'_>],
        declared_keys: &[String],
        metadata_keys: &[String],
        metadata_bytes: &[u8],
        expected_metadata_hash: [u8; 32],
        parse: impl Fn(&[u8]) -> Result<Bundle, RegistrationError>,
    ) -> Result<Self, RegistrationError> {
        Self::register_internal(
            resources,
            declared_keys,
            metadata_keys,
            metadata_bytes,
            expected_metadata_hash,
            true,
            parse,
        )
    }

    fn register_internal(
        resources: &[BundleResource<'_>],
        declared_keys: &[String],
        metadata_keys: &[String],
        metadata_bytes: &[u8],
        expected_metadata_hash: [u8; 32],
        wire_contract: bool,
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
        if blake3::hash(metadata_bytes).as_bytes() != &expected_metadata_hash {
            return Err(RegistrationError);
        }
        unique(metadata_keys.iter().map(String::as_str))?;
        let reason_messages = metadata::parse_reason_messages(metadata_bytes)?;
        let metadata_hash = blake3::hash(metadata_bytes);
        let mut bundles = BTreeMap::new();
        let mut resource_digests = BTreeMap::new();
        let mut bundle_revisions = BTreeMap::new();
        let mut bundle_bodies = BTreeMap::new();
        for resource in checked {
            let bundle = parse(resource.bytes)?;
            if bundle.language != resource.language || bundle.version != resource.version {
                return Err(RegistrationError);
            }
            unique(bundle.messages.iter().map(|(key, _)| key.as_str()))?;
            if wire_contract {
                let revision = bundle_revision(
                    resource.language,
                    &resource.expected_hash,
                    metadata_hash.as_bytes(),
                )
                .ok_or(RegistrationError)?;
                let body =
                    bundle_body::from_verified_bundle(&bundle, &reason_messages, &metadata_hash)?;
                bundle_revisions.insert(bundle.language.clone(), revision);
                bundle_bodies.insert(bundle.language.clone(), body);
            }
            resource_digests.insert(bundle.language.clone(), resource.expected_hash);
            bundles.insert(bundle.language.clone(), bundle);
        }
        unique(declared_keys.iter().map(String::as_str))?;
        let english = bundles.get("en").ok_or(RegistrationError)?;
        let english_keys: BTreeSet<_> = english.messages.iter().map(|(key, _)| key).collect();
        if declared_keys.iter().any(|key| !english_keys.contains(key)) {
            return Err(RegistrationError);
        }
        let declared: BTreeSet<_> = declared_keys.iter().map(String::as_str).collect();
        if wire_contract {
            if !declared.contains(UNKNOWN_REASON_MESSAGE_KEY) {
                return Err(RegistrationError);
            }
            metadata::validate_reason_messages(&reason_messages, &bundles, &declared)?;
        }
        Ok(Self {
            bundles,
            resource_digests,
            reason_messages,
            metadata_revision: metadata_hash.to_hex().to_string(),
            bundle_revisions,
            bundle_bodies,
            configured_languages: resources
                .iter()
                .map(|row| row.language.to_owned())
                .collect(),
            integrity_warnings: warnings,
            missing_messages: missing::MissingMessageQueue::default(),
        })
    }

    /// Digest of exactly the bytes that passed registration, never a reserialization.
    pub fn resource_digest(&self, language: &str) -> Option<[u8; 32]> {
        self.resource_digests.get(language).copied()
    }

    /// Returns an integrity-verified bundle's own language-name key when present.
    pub fn self_name_key(&self, language: &str) -> Option<&str> {
        let key = format!("locale.language.{language}");
        let bundle = self.bundle(language)?;
        bundle.messages.iter().find_map(|(candidate, message)| {
            (candidate == &key && message.parameters.is_empty()).then_some(candidate.as_str())
        })
    }

    /// Returns only integrity-verified languages with stable startup revisions.
    pub fn supported_languages(
        &self,
    ) -> Vec<du_core_contracts::locale_ipc::LocaleSupportedLanguage> {
        self.languages()
            .filter_map(|language| {
                Some(du_core_contracts::locale_ipc::LocaleSupportedLanguage {
                    language: language.to_owned(),
                    self_name_key: self.self_name_key(language)?.to_owned(),
                    bundle_revision: self.bundle_revision(language)?.to_owned(),
                })
            })
            .collect()
    }

    /// Returns the current verified bundle revision for a registered language.
    pub fn bundle_revision(&self, language: &str) -> Option<&str> {
        self.bundle_revisions.get(language).map(String::as_str)
    }

    /// Returns the digest of the exact embedded reason metadata bytes.
    pub fn metadata_revision(&self) -> &str {
        &self.metadata_revision
    }

    /// Builds the IF-02 response from verified startup data.
    pub fn bundle_response(&self, request: &LocaleBundleRequest) -> Option<LocaleBundleResponse> {
        let bundle_revision = self.bundle_revision(&request.language)?;
        let bundle = (request.known_bundle_revision.as_deref() != Some(bundle_revision))
            .then(|| self.bundle_bodies.get(&request.language).cloned())
            .flatten();
        Some(LocaleBundleResponse {
            bundle_revision: bundle_revision.to_owned(),
            bundle,
        })
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
