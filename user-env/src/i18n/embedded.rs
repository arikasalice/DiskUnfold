//! Built-in resource format and startup composition inputs.
//!
//! Each JSON bundle is a six-element array: language, version, format table,
//! plural rule, fallback language, and message rows. Each message row contains
//! key, text, and parameter rows; each parameter row contains name and kind.
//! Arrays retain duplicate keys so registration can reject them. Registration
//! validates the published format table and fallback rules before startup.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{Bundle, BundleResource, Message, ReasonMetadataResource, RegistrationError, Registry};
use du_core_contracts::locale_bundle::LocaleFormatTable;

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
    "settings.scope.recent_limit.name",
    "settings.scope.recent_limit.description",
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
    "ui.locale.title",
    "ui.locale.date_format",
    "ui.locale.number_format",
    "ui.locale.time_zone",
    "ui.locale.motion",
    "ui.locale.scale",
    "ui.locale.language",
    "ui.locale.default",
    "ui.locale.no_grouping",
    "ui.locale.local",
    "ui.locale.follow_os",
    "ui.locale.reduce",
    "ui.locale.loading",
    "ui.locale.failed",
    "ui.locale.save_failed",
    "ui.locale.modified",
    "ui.common.retry",
    "ui.common.close",
    "ui.common.translation_unverified",
    "ui.common.error_generic",
    "value.unknown",
    "reason.locale.unsupported.cause",
    "reason.locale.unsupported.action",
    "reason.locale.format_unknown.cause",
    "reason.locale.format_unknown.action",
    "reason.locale.profile_modified.cause",
    "reason.locale.profile_modified.action",
    "reason.locale.bundle_unavailable.cause",
    "reason.locale.bundle_unavailable.action",
    "reason.i18n.bundle_integrity_failed.action",
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

/// Returns the exact reason metadata bytes and build-time digest.
pub fn reason_metadata_resource() -> Result<ReasonMetadataResource<'static>, RegistrationError> {
    let input: &'static [u8] = include_bytes!(concat!(env!("OUT_DIR"), "/reason-metadata.bin"));
    let (hash, bytes) = input.split_at_checked(32).ok_or(RegistrationError)?;
    Ok(ReasonMetadataResource {
        bytes,
        expected_hash: hash.try_into().map_err(|_| RegistrationError)?,
    })
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
    validate_format_table(&fields[2])?;
    let plural_rule = string(&fields[3])?;
    // The fixed resource row represents the optional English fallback with an empty string.
    let fallback_language = optional_string(&fields[4])?;
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
                .insert(string(&pair[0])?, parameter_kind(&pair[1])?)
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

fn optional_string(value: &Value) -> Result<String, RegistrationError> {
    value.as_str().map(str::to_owned).ok_or(RegistrationError)
}

fn parameter_kind(value: &Value) -> Result<String, RegistrationError> {
    let kind = string(value)?;
    if !matches!(
        kind.as_str(),
        "count" | "size" | "timestamp" | "number" | "path" | "name" | "message_key"
    ) {
        return Err(RegistrationError);
    }
    Ok(kind)
}

fn validate_format_table(value: &Value) -> Result<(), RegistrationError> {
    let fields = value.as_object().ok_or(RegistrationError)?;
    let decimal_separator = string(fields.get("decimalSeparator").ok_or(RegistrationError)?)?;
    let grouping_separator = string(fields.get("groupingSeparator").ok_or(RegistrationError)?)?;
    if decimal_separator == grouping_separator
        || fields
            .get("groupingSize")
            .and_then(Value::as_u64)
            .is_none_or(|size| size == 0)
    {
        return Err(RegistrationError);
    }

    let date_patterns = fields
        .get("datePatterns")
        .and_then(Value::as_object)
        .ok_or(RegistrationError)?;
    if date_patterns.len() != 2
        || ["lang_default", "iso8601"].iter().any(|key| {
            date_patterns
                .get(*key)
                .and_then(Value::as_str)
                .is_none_or(|pattern| {
                    !valid_date_pattern(pattern, &decimal_separator, &grouping_separator)
                })
        })
    {
        return Err(RegistrationError);
    }

    let unit_symbols = fields
        .get("unitSymbols")
        .and_then(Value::as_object)
        .ok_or(RegistrationError)?;
    let required_symbols: BTreeSet<_> = du_core_contracts::size_units::SI_UNITS
        .symbols()
        .iter()
        .chain(du_core_contracts::size_units::IEC_UNITS.symbols())
        .copied()
        .collect();
    if unit_symbols.len() != required_symbols.len()
        || required_symbols
            .iter()
            .any(|symbol| unit_symbols.get(*symbol).and_then(Value::as_str) != Some(*symbol))
    {
        return Err(RegistrationError);
    }
    serde_json::from_value::<LocaleFormatTable>(value.clone()).map_err(|_| RegistrationError)?;
    Ok(())
}

fn valid_date_pattern(pattern: &str, decimal_separator: &str, grouping_separator: &str) -> bool {
    let mut allowed_separators = BTreeSet::from(['-', '/', '.', ':', ' ']);
    allowed_separators.extend(decimal_separator.chars());
    allowed_separators.extend(grouping_separator.chars());

    let mut tokens = BTreeSet::new();
    let mut chars = pattern.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\'' {
            if chars.next() != Some('T') || chars.next() != Some('\'') {
                return false;
            }
        } else if character.is_ascii_alphabetic() {
            let mut token = String::from(character);
            while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                if let Some(next) = chars.next() {
                    token.push(next);
                }
            }
            if !matches!(token.as_str(), "yyyy" | "MM" | "dd" | "HH" | "mm" | "ss")
                || !tokens.insert(token)
            {
                return false;
            }
        } else if !allowed_separators.contains(&character) {
            return false;
        }
    }
    tokens.len() == 6
        && ["yyyy", "MM", "dd", "HH", "mm", "ss"]
            .iter()
            .all(|token| tokens.contains(*token))
}

/// Perform the normal startup registration using the existing registry contract.
pub fn register(metadata_keys: &[String]) -> Result<Registry, RegistrationError> {
    register_resources(&resources()?, metadata_keys)
}

fn register_resources(
    resources: &[BundleResource<'_>],
    metadata_keys: &[String],
) -> Result<Registry, RegistrationError> {
    let metadata = reason_metadata_resource()?;
    register_resources_with_metadata(
        resources,
        metadata_keys,
        metadata.bytes,
        metadata.expected_hash,
    )
}

fn register_resources_with_metadata(
    resources: &[BundleResource<'_>],
    metadata_keys: &[String],
    metadata_bytes: &[u8],
    expected_metadata_hash: [u8; 32],
) -> Result<Registry, RegistrationError> {
    let declared = DECLARED_KEYS
        .iter()
        .map(|key| (*key).to_owned())
        .collect::<Vec<_>>();
    let registry = Registry::register_with_reason_metadata(
        resources,
        &declared,
        metadata_keys,
        metadata_bytes,
        expected_metadata_hash,
        parse_bundle,
    )?;
    validate_fallbacks(&registry)?;
    for language in registry.languages() {
        registry.self_name_key(language).ok_or(RegistrationError)?;
    }
    let languages: Vec<_> = registry.languages().collect();
    let descriptors =
        crate::settings::builtin_descriptors(&languages).map_err(|_| RegistrationError)?;
    validate_setting_presentation(&registry, descriptors)?;
    Ok(registry)
}

fn validate_fallbacks(registry: &Registry) -> Result<(), RegistrationError> {
    let english = registry.bundle("en").ok_or(RegistrationError)?;
    if !english.fallback_language.is_empty() {
        return Err(RegistrationError);
    }
    if registry
        .bundle("ko")
        .is_some_and(|korean| korean.fallback_language != "en")
    {
        return Err(RegistrationError);
    }

    for language in registry.languages().filter(|language| *language != "en") {
        let mut current = language;
        let mut seen = std::collections::BTreeSet::new();
        loop {
            if !seen.insert(current) {
                return Err(RegistrationError);
            }
            let bundle = registry.bundle(current).ok_or(RegistrationError)?;
            let fallback = bundle.fallback_language.as_str();
            if fallback.is_empty() || fallback == current {
                return Err(RegistrationError);
            }
            if fallback == "en" {
                break;
            }
            if registry.bundle(fallback).is_none() {
                return Err(RegistrationError);
            }
            current = fallback;
        }
    }
    Ok(())
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
