//! Parsing and validation of the exact embedded reason metadata table.

use std::{collections::BTreeMap, fmt};

use du_core_contracts::locale_bundle::ReasonMessageMeta;
use serde::de::{MapAccess, Visitor};

use super::{Bundle, RegistrationError, UNKNOWN_REASON_MESSAGE_KEY, bundle_body::find_message};

pub(super) fn parse_reason_messages(
    bytes: &[u8],
) -> Result<BTreeMap<String, ReasonMessageMeta>, RegistrationError> {
    struct UniqueReasonMessages(BTreeMap<String, ReasonMessageMeta>);

    impl<'de> serde::Deserialize<'de> for UniqueReasonMessages {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct UniqueMapVisitor;

            impl<'de> Visitor<'de> for UniqueMapVisitor {
                type Value = UniqueReasonMessages;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("an object with unique reason keys")
                }

                fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                    let mut entries = BTreeMap::new();
                    while let Some((key, value)) = map.next_entry::<String, ReasonMessageMeta>()? {
                        if key.trim().is_empty() || entries.insert(key, value).is_some() {
                            return Err(serde::de::Error::custom(
                                "reason keys must be nonempty and unique",
                            ));
                        }
                    }
                    Ok(UniqueReasonMessages(entries))
                }
            }

            deserializer.deserialize_map(UniqueMapVisitor)
        }
    }

    serde_json::from_slice::<UniqueReasonMessages>(bytes)
        .map(|parsed| parsed.0)
        .map_err(|_| RegistrationError)
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
