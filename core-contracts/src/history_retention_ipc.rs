//! GUI contracts for reading and changing the stored history retention policy.

use crate::Id;
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

/// A retention-policy transition. Consumers should show a generic transition
/// when they receive a future value they do not recognize.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum HistoryRetentionChangeKind {
    None,
    Enable,
    Shorten,
    Extend,
    Disable,
    SetPeriod,
}

impl HistoryRetentionChangeKind {
    /// Returns the stable wire value used in a retention-review fingerprint.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Enable => "enable",
            Self::Shorten => "shorten",
            Self::Extend => "extend",
            Self::Disable => "disable",
            Self::SetPeriod => "set_period",
        }
    }
}

/// Requests a review of the policy that would be stored if applied.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryRetentionReviewRequest {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "number"))]
    pub days: Option<u32>,
}

impl<'de> Deserialize<'de> for HistoryRetentionReviewRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            enabled: bool,
            #[serde(default, deserialize_with = "optional_present")]
            days: Option<u32>,
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.days == Some(0) {
            return Err(serde::de::Error::custom("days must be at least one"));
        }
        Ok(Self {
            enabled: wire.enabled,
            days: wire.days,
        })
    }
}

/// Requests application of one previously reviewed policy transition.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryRetentionApplyRequest {
    pub review_ref: Id,
    #[serde(deserialize_with = "review_fingerprint")]
    pub fingerprint: String,
}

fn review_fingerprint<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if is_fingerprint(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "fingerprint must be 32 lowercase hexadecimal digits",
        ))
    }
}

fn is_fingerprint(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Requests cancellation of one in-memory retention review.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryRetentionCancelRequest {
    pub review_ref: Id,
}

/// Counts and range for applying a retention-policy review.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryRetentionReview {
    pub review_ref: Id,
    pub change_kind: HistoryRetentionChangeKind,
    pub confirmation_required: bool,
    pub cleanup_after_apply: bool,
    pub target_count: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "number | null"))]
    pub target_earliest_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "number | null"))]
    pub target_latest_at_ms: Option<i64>,
    pub held_count: String,
    pub undo_info_removal_count: String,
    pub fingerprint: String,
}

impl<'de> Deserialize<'de> for HistoryRetentionReview {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            review_ref: Id,
            change_kind: HistoryRetentionChangeKind,
            confirmation_required: bool,
            cleanup_after_apply: bool,
            target_count: String,
            #[serde(default, deserialize_with = "optional_present")]
            target_earliest_at_ms: Option<i64>,
            #[serde(default, deserialize_with = "optional_present")]
            target_latest_at_ms: Option<i64>,
            held_count: String,
            undo_info_removal_count: String,
            #[serde(deserialize_with = "review_fingerprint")]
            fingerprint: String,
        }

        let wire = Wire::deserialize(deserializer)?;
        if !is_decimal_count(&wire.target_count)
            || !is_decimal_count(&wire.held_count)
            || !is_decimal_count(&wire.undo_info_removal_count)
        {
            return Err(serde::de::Error::custom(
                "retention counts must be canonical decimal strings",
            ));
        }
        if wire.target_count == "0" {
            if wire.target_earliest_at_ms.is_some() || wire.target_latest_at_ms.is_some() {
                return Err(serde::de::Error::custom(
                    "empty target ranges must be omitted",
                ));
            }
        } else if wire.target_earliest_at_ms.is_none() || wire.target_latest_at_ms.is_none() {
            return Err(serde::de::Error::custom(
                "nonempty target ranges require both endpoints",
            ));
        }
        Ok(Self {
            review_ref: wire.review_ref,
            change_kind: wire.change_kind,
            confirmation_required: wire.confirmation_required,
            cleanup_after_apply: wire.cleanup_after_apply,
            target_count: wire.target_count,
            target_earliest_at_ms: wire.target_earliest_at_ms,
            target_latest_at_ms: wire.target_latest_at_ms,
            held_count: wire.held_count,
            undo_info_removal_count: wire.undo_info_removal_count,
            fingerprint: wire.fingerprint,
        })
    }
}

fn is_decimal_count(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|count| count.to_string() == value)
}

/// The result of applying or reusing a retention-policy review.
/// Consumers should handle future outcome values as a generic applied result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum HistoryRetentionApplyOutcome {
    Applied,
    NoEffect,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryRetentionApplyResponse {
    pub outcome: HistoryRetentionApplyOutcome,
    pub policy: HistoryRetentionPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub cleanup_job_ref: Option<Id>,
}

impl<'de> Deserialize<'de> for HistoryRetentionApplyResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            outcome: HistoryRetentionApplyOutcome,
            policy: HistoryRetentionPolicy,
            #[serde(default, deserialize_with = "optional_present")]
            cleanup_job_ref: Option<Id>,
        }

        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            outcome: wire.outcome,
            policy: wire.policy,
            cleanup_job_ref: wire.cleanup_job_ref,
        })
    }
}

/// The result of cancelling a retention-policy review.
/// Consumers should handle future outcome values as a generic cancellation result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum HistoryRetentionCancelOutcome {
    Cancelled,
    NoEffect,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryRetentionCancelResponse {
    pub outcome: HistoryRetentionCancelOutcome,
}

#[cfg(test)]
mod tests;
