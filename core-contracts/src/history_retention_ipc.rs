//! GUI contract for reading the stored history retention policy.

use serde::{Deserialize, Deserializer, Serialize};

/// An empty request for the history retention policy.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct HistoryRetentionGetRequest {}

impl<'de> Deserialize<'de> for HistoryRetentionGetRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.as_object().is_some_and(serde_json::Map::is_empty) {
            Ok(Self {})
        } else {
            Err(serde::de::Error::custom("expected an empty object"))
        }
    }
}

/// Effective history retention values read from one settings snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryRetentionPolicy {
    /// Whether automatic retention is enabled.
    pub enabled: bool,
    /// The user-specified age limit, omitted when no override is stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub specified_days: Option<u32>,
    /// Built-in suggestion, which is never stored as a setting.
    pub suggested_days: u32,
    /// Lowercase hexadecimal digest that identifies this policy and its read state.
    pub policy_fingerprint: String,
    /// Stable reason key when the stored policy could not be trusted.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub unavailable_reason: Option<String>,
}

impl<'de> Deserialize<'de> for HistoryRetentionPolicy {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            enabled: bool,
            #[serde(default, deserialize_with = "optional_present")]
            specified_days: Option<u32>,
            suggested_days: u32,
            policy_fingerprint: String,
            #[serde(default, deserialize_with = "optional_present")]
            unavailable_reason: Option<String>,
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.specified_days == Some(0) {
            return Err(serde::de::Error::custom(
                "specifiedDays must be at least one",
            ));
        }
        if wire.policy_fingerprint.len() != 32
            || !wire
                .policy_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(serde::de::Error::custom(
                "policyFingerprint must be 32 lowercase hexadecimal digits",
            ));
        }
        if wire.unavailable_reason.is_some() && (wire.enabled || wire.specified_days.is_some()) {
            return Err(serde::de::Error::custom(
                "an unavailable policy must be disabled and have no specifiedDays",
            ));
        }
        Ok(Self {
            enabled: wire.enabled,
            specified_days: wire.specified_days,
            suggested_days: wire.suggested_days,
            policy_fingerprint: wire.policy_fingerprint,
            unavailable_reason: wire.unavailable_reason,
        })
    }
}

fn optional_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn valid_policy() -> Value {
        json!({
            "enabled": true,
            "specifiedDays": 180,
            "suggestedDays": 90,
            "policyFingerprint": "0123456789abcdef0123456789abcdef"
        })
    }

    #[test]
    fn request_requires_an_empty_object() {
        assert!(serde_json::from_value::<HistoryRetentionGetRequest>(json!({})).is_ok());
        for value in [json!({"extra": 1}), json!(null), json!([]), json!(0)] {
            assert!(serde_json::from_value::<HistoryRetentionGetRequest>(value).is_err());
        }
    }

    #[test]
    fn policy_round_trips_with_omitted_optional_fields() -> Result<(), serde_json::Error> {
        let value = valid_policy();
        let policy: HistoryRetentionPolicy = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(policy)?, value);
        Ok(())
    }

    #[test]
    fn policy_accepts_unknown_response_fields() {
        let mut response = valid_policy();
        response["futureField"] = json!(true);
        assert!(serde_json::from_value::<HistoryRetentionPolicy>(response).is_ok());
    }

    #[test]
    fn policy_rejects_invalid_optional_fields_and_fingerprints() {
        let mut invalid = valid_policy();
        invalid["specifiedDays"] = Value::Null;
        assert!(serde_json::from_value::<HistoryRetentionPolicy>(invalid).is_err());

        let mut invalid = valid_policy();
        invalid["unavailableReason"] = Value::Null;
        assert!(serde_json::from_value::<HistoryRetentionPolicy>(invalid).is_err());

        let mut invalid = valid_policy();
        invalid["specifiedDays"] = json!(0);
        assert!(serde_json::from_value::<HistoryRetentionPolicy>(invalid).is_err());

        for fingerprint in ["", "0123456789ABCDEF0123456789ABCDEF", "0123456789abcdef"] {
            let mut invalid = valid_policy();
            invalid["policyFingerprint"] = json!(fingerprint);
            assert!(serde_json::from_value::<HistoryRetentionPolicy>(invalid).is_err());
        }

        for (enabled, days) in [(true, None), (false, Some(180))] {
            let mut invalid = json!({
                "enabled": enabled,
                "suggestedDays": 90,
                "policyFingerprint": "0123456789abcdef0123456789abcdef",
                "unavailableReason": "history.policy_invalid"
            });
            if let Some(days) = days {
                invalid["specifiedDays"] = json!(days);
            }
            assert!(serde_json::from_value::<HistoryRetentionPolicy>(invalid).is_err());
        }
    }
}
