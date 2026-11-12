//! Built-in resource format and startup composition inputs.
//!
//! Each JSON bundle is a six-element array: language, version, format table,
//! plural rule, fallback language, and message rows. Each message row contains
//! key, text, and parameter rows; each parameter row contains name and kind.
//! Arrays retain duplicate keys so registration can reject them. The empty format
//! tables are placeholders until the separate formatter work supplies its data.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Bundle, BundleResource, Message, RegistrationError, Registry};

pub const DECLARED_KEYS: &[&str] = &[
    "ui.product.name",
    "locale.language.en",
    "locale.language.ko",
    "cli.help.usage",
    "cli.help.unsupported",
    "cli.settings.unavailable",
    "cli.output.interrupted",
    "locale.registration_failed",
    "i18n.bundle_integrity_failed",
    "settings.scope.remember_recent.name",
    "settings.scope.remember_recent.description",
    "settings.display.size_unit_system.name",
    "settings.display.size_unit_system.description",
    "settings.display.size_unit.name",
    "settings.display.size_unit.description",
    "settings.space.default_basis.name",
    "settings.space.default_basis.description",
    "settings.space.shared_table_max_entries.name",
    "settings.space.shared_table_max_entries.description",
    "settings.locale.display_language.name",
    "settings.locale.display_language.description",
    "settings.locale.date_format.name",
    "settings.locale.date_format.description",
    "settings.locale.number_format.name",
    "settings.locale.number_format.description",
    "settings.locale.time_zone_policy.name",
    "settings.locale.time_zone_policy.description",
    "settings.a11y.motion.name",
    "settings.a11y.motion.description",
    "settings.a11y.app_scale.name",
    "settings.a11y.app_scale.description",
];

/// Resource rows and bytes come from one build-generated, immutable archive.
pub fn resources() -> Result<Vec<BundleResource<'static>>, RegistrationError> {
    let mut input: &'static [u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bundles.bin"));
    let mut resources = Vec::new();
    while !input.is_empty() {
        let language = std::str::from_utf8(field(&mut input)?).map_err(|_| RegistrationError)?;
        let version = std::str::from_utf8(field(&mut input)?).map_err(|_| RegistrationError)?;
        let expected_hash = field(&mut input)?
            .try_into()
            .map_err(|_| RegistrationError)?;
        let bytes = field(&mut input)?;
        resources.push(BundleResource {
            language,
            version,
            bytes,
            expected_hash,
        });
    }
    Ok(resources)
}

fn field<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], RegistrationError> {
    let header: [u8; 4] = input
        .get(..4)
        .ok_or(RegistrationError)?
        .try_into()
        .map_err(|_| RegistrationError)?;
    let length = usize::try_from(u32::from_le_bytes(header)).map_err(|_| RegistrationError)?;
    let end = 4_usize.checked_add(length).ok_or(RegistrationError)?;
    let value = input.get(4..end).ok_or(RegistrationError)?;
    *input = input.get(end..).ok_or(RegistrationError)?;
    Ok(value)
}
/// Parse only the verified bytes; callers must still register and check the hash.
pub fn parse_bundle(bytes: &[u8]) -> Result<Bundle, RegistrationError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| RegistrationError)?;
    let fields = row(&value, 6)?;
    let language = string(&fields[0])?;
    let version = string(&fields[1])?;
    if !fields[2].is_object() {
        return Err(RegistrationError);
    }
    let plural_rule = string(&fields[3])?;
    let fallback_language = string(&fields[4])?;
    let rows = fields[5].as_array().ok_or(RegistrationError)?;
    let mut messages = Vec::with_capacity(rows.len());
    for value in rows {
        let fields = row(value, 3)?;
        let key = string(&fields[0])?;
        let text = string(&fields[1])?;
        let entries = fields[2].as_array().ok_or(RegistrationError)?;
        let mut parameters = BTreeMap::new();
        for entry in entries {
            let pair = row(entry, 2)?;
            if parameters
                .insert(string(&pair[0])?, string(&pair[1])?)
                .is_some()
            {
                return Err(RegistrationError);
            }
        }
        messages.push((key, Message { text, parameters }));
    }
    Ok(Bundle {
        language,
        version,
        messages,
        format_table: fields[2].clone(),
        plural_rule,
        fallback_language,
    })
}

fn row(value: &Value, length: usize) -> Result<&[Value], RegistrationError> {
    let values = value.as_array().ok_or(RegistrationError)?;
    if values.len() != length {
        return Err(RegistrationError);
    }
    Ok(values)
}

fn string(value: &Value) -> Result<String, RegistrationError> {
    let value = value
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or(RegistrationError)?;
    Ok(value.to_owned())
}

/// Perform the normal startup registration using the existing registry contract.
pub fn register(metadata_keys: &[String]) -> Result<Registry, RegistrationError> {
    let declared = DECLARED_KEYS
        .iter()
        .map(|key| (*key).to_owned())
        .collect::<Vec<_>>();
    let registry = Registry::register(&resources()?, &declared, metadata_keys, parse_bundle)?;
    for language in registry.languages() {
        registry.self_name_key(language).ok_or(RegistrationError)?;
    }
    let languages: Vec<_> = registry.configured_languages().collect();
    let descriptors =
        crate::settings::builtin_descriptors(&languages).map_err(|_| RegistrationError)?;
    validate_setting_presentation(&registry, descriptors)?;
    Ok(registry)
}

/// Validates unique presentation keys against every available verified bundle.
fn validate_setting_presentation(
    registry: &Registry,
    descriptors: impl IntoIterator<Item = crate::Descriptor>,
) -> Result<(), RegistrationError> {
    let mut keys = std::collections::BTreeSet::new();
    for descriptor in descriptors {
        for key in [
            descriptor.presentation.name_key,
            descriptor.presentation.description_key,
        ] {
            if key.is_empty() || !keys.insert(key.clone()) {
                return Err(RegistrationError);
            }
            for language in registry.languages() {
                let bundle = registry.bundle(language).ok_or(RegistrationError)?;
                if !bundle
                    .messages
                    .iter()
                    .any(|(candidate, _)| candidate == &key)
                {
                    return Err(RegistrationError);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
