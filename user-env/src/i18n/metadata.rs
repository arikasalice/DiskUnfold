//! Parsing and validation of the exact embedded reason metadata table.

use std::collections::BTreeMap;

use du_core_contracts::locale_bundle::ReasonMessageMeta;

use super::{Bundle, RegistrationError, UNKNOWN_REASON_MESSAGE_KEY, bundle_body::find_message};

pub(super) fn parse_reason_messages(
    bytes: &[u8],
) -> Result<BTreeMap<String, ReasonMessageMeta>, RegistrationError> {
    // Rows preserve duplicates for validation before building the wire map.
    let rows: Vec<(String, ReasonMessageMeta)> =
        serde_json::from_slice(bytes).map_err(|_| RegistrationError)?;
    let mut entries = BTreeMap::new();
    for (key, value) in rows {
        if key.trim().is_empty() || entries.insert(key, value).is_some() {
            return Err(RegistrationError);
        }
    }
    Ok(entries)
}

pub(super) fn validate_reason_messages(
    reason_messages: &BTreeMap<String, ReasonMessageMeta>,
    bundles: &BTreeMap<String, Bundle>,
    declared: &std::collections::BTreeSet<&str>,
) -> Result<(), RegistrationError> {
    let english = bundles.get("en").ok_or(RegistrationError)?;
    for required_key in [UNKNOWN_REASON_MESSAGE_KEY, "value.unknown"] {
        if !declared.contains(required_key) || find_message(english, required_key).is_none() {
            return Err(RegistrationError);
        }
    }
    for metadata in reason_messages.values() {
        if metadata.cause_key.trim().is_empty()
            || metadata.action_key.trim().is_empty()
            || !declared.contains(metadata.cause_key.as_str())
            || !declared.contains(metadata.action_key.as_str())
        {
            return Err(RegistrationError);
        }
        for bundle in bundles.values() {
            if find_message(bundle, &metadata.cause_key).is_none()
                || find_message(bundle, &metadata.action_key).is_none()
            {
                return Err(RegistrationError);
            }
        }
    }
    Ok(())
}
