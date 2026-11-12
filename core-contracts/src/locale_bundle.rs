//! F13 language bundle and missing translation IPC bodies.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, de::Error};

/// The value kind of a named message template parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum MessageParameterKind {
    /// Integral item count.
    Count,
    /// Byte size or formatted size.
    Size,
    /// Timestamp value.
    Timestamp,
    /// Numeric value.
    Number,
    /// Original path value.
    Path,
    /// Original file or item name.
    Name,
    /// Nested message key.
    MessageKey,
}

/// Typed raw input supplied to a human-readable message renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocaleMessageValue<'a> {
    /// Count used for display or plural selection.
    Count(u64),
    /// Original unsigned byte count.
    Size(u64),
    /// Original UTC timestamp string.
    Timestamp(&'a str),
    /// Exact decimal coefficient and decimal scale.
    Number { coefficient: i128, scale: u8 },
    /// Original path presentation string.
    Path(&'a str),
    /// Original item name or already prepared display label.
    Name(&'a str),
    /// Nested plain message key, resolved once.
    MessageKey(&'a str),
}

impl LocaleMessageValue<'_> {
    /// Kind checked against the template's declared parameter type.
    pub const fn kind(self) -> MessageParameterKind {
        match self {
            Self::Count(_) => MessageParameterKind::Count,
            Self::Size(_) => MessageParameterKind::Size,
            Self::Timestamp(_) => MessageParameterKind::Timestamp,
            Self::Number { .. } => MessageParameterKind::Number,
            Self::Path(_) => MessageParameterKind::Path,
            Self::Name(_) => MessageParameterKind::Name,
            Self::MessageKey(_) => MessageParameterKind::MessageKey,
        }
    }
}

/// One localized message template and its declared parameters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageTemplate {
    /// A text template.
    Text {
        /// Named parameters and their types.
        parameters: BTreeMap<String, MessageParameterKind>,
        /// Template text using the registered template syntax.
        template: String,
    },
    /// A template selected by the named count parameter's plural category.
    Plural {
        /// Named parameters and their types.
        parameters: BTreeMap<String, MessageParameterKind>,
        /// Parameter whose value selects a variant.
        parameter: String,
        /// Plural category to template text.
        variants: BTreeMap<String, String>,
    },
}

/// Date pattern identifier used by a locale bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocaleDatePattern {
    /// Language-specific default pattern.
    LangDefault,
    /// ISO 8601 pattern.
    Iso8601,
}

/// Locale-specific symbols and patterns used only when rendering values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleFormatTable {
    /// Decimal separator symbol.
    pub decimal_separator: String,
    /// Digit grouping separator symbol.
    pub grouping_separator: String,
    /// Positive number of digits in each grouping segment.
    pub grouping_size: u32,
    /// Date patterns by published format identifier.
    pub date_patterns: BTreeMap<LocaleDatePattern, String>,
    /// Symbols indexed by the published F03 unit identifiers.
    pub unit_symbols: BTreeMap<String, String>,
}

impl<'de> Deserialize<'de> for LocaleFormatTable {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            decimal_separator: String,
            grouping_separator: String,
            grouping_size: u32,
            date_patterns: BTreeMap<LocaleDatePattern, String>,
            unit_symbols: BTreeMap<String, String>,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("locale format table must be an object"));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            decimal_separator: wire.decimal_separator,
            grouping_separator: wire.grouping_separator,
            grouping_size: wire.grouping_size,
            date_patterns: wire.date_patterns,
            unit_symbols: wire.unit_symbols,
        })
    }
}

/// Message keys and language-neutral attributes for an owner-defined reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReasonMessageMeta {
    /// Message key describing the cause.
    pub cause_key: String,
    /// Message key describing the follow-up action.
    pub action_key: String,
    /// Whether the represented operation cannot be reversed.
    pub irreversible: bool,
    /// Evidence certainty associated with the reason.
    pub certainty: ReasonCertainty,
    /// User-visible lifecycle state associated with the reason.
    pub status: ReasonStatus,
}

impl<'de> Deserialize<'de> for ReasonMessageMeta {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            cause_key: String,
            action_key: String,
            irreversible: bool,
            certainty: ReasonCertainty,
            status: ReasonStatus,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("reason metadata must be an object"));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            cause_key: wire.cause_key,
            action_key: wire.action_key,
            irreversible: wire.irreversible,
            certainty: wire.certainty,
            status: wire.status,
        })
    }
}

/// Published certainty values for reason metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ReasonCertainty {
    /// Evidence confirms the represented conclusion.
    Confirmed,
    /// Evidence is incomplete and cannot establish a conclusion.
    Uncertain,
    /// No evidence is available to judge certainty.
    Unknown,
    /// Certainty does not apply to this state or guidance.
    NotApplicable,
}

/// Published status values for reason metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ReasonStatus {
    /// Normal state.
    Normal,
    /// Work is in progress.
    InProgress,
    /// Work is complete.
    Complete,
    /// No matching content is available.
    Empty,
    /// The operation was denied by permission.
    PermissionDenied,
    /// The connection is unavailable.
    Disconnected,
    /// The result is incomplete.
    Incomplete,
    /// User confirmation is required.
    Confirmation,
    /// The operation was cancelled.
    Cancelled,
    /// The operation failed.
    Failed,
}

/// Wire body containing one integrity-verified language bundle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleBundleBody {
    /// Message templates keyed by stable message identifier.
    pub messages: BTreeMap<String, MessageTemplate>,
    /// Locale-specific formatting rules.
    pub format_table: LocaleFormatTable,
    /// Registered plural rule identifier.
    pub plural_rule: String,
    /// Language used after this bundle when a message is missing.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_present"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub fallback_language: Option<String>,
    /// Owner-defined, language-neutral reason metadata.
    pub reason_messages: BTreeMap<String, ReasonMessageMeta>,
    /// Lowercase BLAKE3 digest of the exact metadata source bytes.
    pub metadata_revision: String,
}

impl<'de> Deserialize<'de> for LocaleBundleBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            messages: BTreeMap<String, MessageTemplate>,
            format_table: LocaleFormatTable,
            plural_rule: String,
            #[serde(default, deserialize_with = "optional_present")]
            fallback_language: Option<String>,
            reason_messages: BTreeMap<String, ReasonMessageMeta>,
            metadata_revision: String,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("locale bundle body must be an object"));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            messages: wire.messages,
            format_table: wire.format_table,
            plural_rule: wire.plural_rule,
            fallback_language: wire.fallback_language,
            reason_messages: wire.reason_messages,
            metadata_revision: wire.metadata_revision,
        })
    }
}

/// Request body for `i18n.get_bundle`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleBundleRequest {
    /// Registered language identifier requested by the caller.
    pub language: String,
    /// The opaque revision for the same-language bundle already held by the caller.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub known_bundle_revision: Option<String>,
}

impl<'de> Deserialize<'de> for LocaleBundleRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            language: String,
            #[serde(default, deserialize_with = "optional_present")]
            known_bundle_revision: Option<String>,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("locale bundle request must be an object"));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            language: wire.language,
            known_bundle_revision: wire.known_bundle_revision,
        })
    }
}

/// Response body for `i18n.get_bundle`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleBundleResponse {
    /// Current opaque revision for the requested language.
    pub bundle_revision: String,
    /// Present unless the request already held this exact revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub bundle: Option<LocaleBundleBody>,
}

impl<'de> Deserialize<'de> for LocaleBundleResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            bundle_revision: String,
            #[serde(default, deserialize_with = "optional_present")]
            bundle: Option<LocaleBundleBody>,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("locale bundle response must be an object"));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            bundle_revision: wire.bundle_revision,
            bundle: wire.bundle,
        })
    }
}

/// Why the displayed message is considered missing for a selected language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocaleMissingMarker {
    /// The English fallback message was displayed.
    Fallback,
    /// The stable message key was displayed.
    Key,
}

/// One missing translation reported by the GUI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleMissingEntry {
    /// Stable message key that was missing.
    pub key: String,
    /// Fallback used or key displayed.
    pub marker: LocaleMissingMarker,
}

impl<'de> Deserialize<'de> for LocaleMissingEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            key: String,
            marker: LocaleMissingMarker,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("missing locale entry must be an object"));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            key: wire.key,
            marker: wire.marker,
        })
    }
}

/// Request body for the best-effort `i18n.report_missing` command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleReportMissingRequest {
    /// Registered language identifier in which the message was missing.
    pub language: String,
    /// Keys and missing markers reported in this batch.
    pub entries: Vec<LocaleMissingEntry>,
}

impl<'de> Deserialize<'de> for LocaleReportMissingRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            language: String,
            entries: Vec<LocaleMissingEntry>,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom(
                "locale missing report request must be an object",
            ));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            language: wire.language,
            entries: wire.entries,
        })
    }
}

/// Empty success body for `i18n.report_missing`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "Record<string, never>"))]
pub struct LocaleReportMissingResponse {}

impl<'de> Deserialize<'de> for LocaleReportMissingResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.is_object() {
            Ok(Self {})
        } else {
            Err(D::Error::custom("expected an object"))
        }
    }
}

#[cfg(test)]
mod tests;

/// Deserialize an optional field while rejecting explicit JSON null.
fn optional_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
