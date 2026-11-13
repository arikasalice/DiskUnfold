//! F13 locale profile request, response, and event bodies.
use serde::{Deserialize, Deserializer, Serialize, de::Error};

/// Canonical decimal string identifying a locale profile snapshot revision.
///
/// Revisions are encoded as strings so their full `u64` range is preserved in
/// JSON and TypeScript clients do not lose precision.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub struct LocaleProfileRevision(u64);

impl LocaleProfileRevision {
    /// Builds a revision from its exact unsigned integer value.
    pub fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Parses a canonical, unsigned decimal revision string.
    pub fn parse(value: &str) -> Result<Self, String> {
        let revision = value
            .parse::<u64>()
            .map_err(|_| "invalid locale profile revision".to_owned())?;
        if revision.to_string() != value {
            return Err("invalid locale profile revision".to_owned());
        }
        Ok(Self(revision))
    }

    /// Returns the exact numeric revision value.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Serialize for LocaleProfileRevision {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for LocaleProfileRevision {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Source used to select the startup display language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocaleLanguageSource {
    /// The language was explicitly selected and saved by the user.
    Stored,
    /// The language was selected from the operating system language.
    Os,
    /// The baseline language was selected.
    Baseline,
}

/// Date format selected for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocaleDateFormat {
    /// Use the selected language's default date format.
    LangDefault,
    /// Use the ISO 8601 date format.
    Iso8601,
}

/// Number format selected for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocaleNumberFormat {
    /// Use the selected language's default number format.
    LangDefault,
    /// Do not group digits.
    NoGrouping,
}

/// Time zone policy selected for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocaleTimeZonePolicy {
    /// Display timestamps in the identified local time zone.
    Local,
    /// Display timestamps in UTC.
    Utc,
}

/// Whether the operating system time zone was identified or only observed as an offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocalTimeZoneStatus {
    /// The operating system time zone identifier was obtained.
    Identified,
    /// Only the current UTC offset was observed.
    OffsetOnly,
    /// The local time zone could not be observed.
    Unavailable,
}

/// Application motion preference stored in the locale profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocaleMotion {
    /// Follow the operating system's motion preference.
    FollowOs,
    /// Reduce application motion.
    Reduce,
}

/// An allowed application display scale, expressed as an integer percentage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "100 | 110 | 125 | 150 | 175 | 200"))]
#[serde(transparent)]
pub struct LocaleAppScale(u16);

impl LocaleAppScale {
    /// Supported display scales in percent.
    pub const ALLOWED_PERCENTAGES: [u16; 6] = [100, 110, 125, 150, 175, 200];

    /// Creates a scale only when it is in the published set.
    pub fn new(percent: u16) -> Option<Self> {
        Self::ALLOWED_PERCENTAGES
            .contains(&percent)
            .then_some(Self(percent))
    }

    /// Returns the scale in percent.
    pub fn percent(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for LocaleAppScale {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let percent = u16::deserialize(deserializer)?;
        Self::new(percent).ok_or_else(|| D::Error::custom("unsupported locale app scale"))
    }
}

/// One registered language offered by the profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleSupportedLanguage {
    /// Registered language identifier.
    pub language: String,
    /// Message key for the language's self-name.
    pub self_name_key: String,
    /// Opaque revision of this language's bundle.
    pub bundle_revision: String,
}

/// The complete locale profile returned by `i18n.get_profile` and
/// `i18n.set_profile`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleProfile {
    /// Selected registered language identifier.
    pub display_language: String,
    /// Source that selected the startup display language.
    pub language_source: LocaleLanguageSource,
    /// Selected date format.
    pub date_format: LocaleDateFormat,
    /// Selected number format.
    pub number_format: LocaleNumberFormat,
    /// Selected timestamp time zone policy.
    pub time_zone_policy: LocaleTimeZonePolicy,
    /// Observed status of the local time zone.
    pub local_time_zone_status: LocalTimeZoneStatus,
    /// Selected application motion preference.
    pub motion: LocaleMotion,
    /// Selected application display scale in percent.
    pub app_scale: LocaleAppScale,
    /// Canonical decimal profile snapshot revision.
    pub profile_revision: LocaleProfileRevision,
    /// Supported languages ordered by language identifier.
    pub supported_languages: Vec<LocaleSupportedLanguage>,
}

impl<'de> Deserialize<'de> for LocaleProfile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            display_language: String,
            language_source: LocaleLanguageSource,
            date_format: LocaleDateFormat,
            number_format: LocaleNumberFormat,
            time_zone_policy: LocaleTimeZonePolicy,
            local_time_zone_status: LocalTimeZoneStatus,
            motion: LocaleMotion,
            app_scale: LocaleAppScale,
            profile_revision: LocaleProfileRevision,
            supported_languages: Vec<LocaleSupportedLanguage>,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("expected a locale profile object"));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        if wire
            .supported_languages
            .windows(2)
            .any(|pair| pair[0].language >= pair[1].language)
        {
            return Err(D::Error::custom(
                "supported languages must be unique and ordered by language ID",
            ));
        }
        Ok(Self {
            display_language: wire.display_language,
            language_source: wire.language_source,
            date_format: wire.date_format,
            number_format: wire.number_format,
            time_zone_policy: wire.time_zone_policy,
            local_time_zone_status: wire.local_time_zone_status,
            motion: wire.motion,
            app_scale: wire.app_scale,
            profile_revision: wire.profile_revision,
            supported_languages: wire.supported_languages,
        })
    }
}

/// A non-null optional field in an update body: omission is represented by
/// `None`, while an explicit JSON `null` is rejected.
fn optional_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// The locale profile fields that may be changed by `i18n.set_profile`.
///
/// Format and setting values remain raw in the Rust request so the app-host
/// can map unknown values to their published locale error reasons. Generated
/// TypeScript still exposes the published closed value unions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleProfileChanges {
    /// New registered language identifier, when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub display_language: Option<String>,
    /// New date format, when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "LocaleDateFormat"))]
    pub date_format: Option<String>,
    /// New number format, when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "LocaleNumberFormat"))]
    pub number_format: Option<String>,
    /// New time zone policy, when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "LocaleTimeZonePolicy"))]
    pub time_zone_policy: Option<String>,
    /// New application motion preference, when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "LocaleMotion"))]
    pub motion: Option<String>,
    /// New application display scale in percent, when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional, type = "LocaleAppScale"))]
    pub app_scale: Option<u16>,
}

impl LocaleProfileChanges {
    /// Returns whether no change field was supplied.
    pub fn is_empty(&self) -> bool {
        self.display_language.is_none()
            && self.date_format.is_none()
            && self.number_format.is_none()
            && self.time_zone_policy.is_none()
            && self.motion.is_none()
            && self.app_scale.is_none()
    }
}

impl<'de> Deserialize<'de> for LocaleProfileChanges {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(default, deserialize_with = "optional_present")]
            display_language: Option<String>,
            #[serde(default, deserialize_with = "optional_present")]
            date_format: Option<String>,
            #[serde(default, deserialize_with = "optional_present")]
            number_format: Option<String>,
            #[serde(default, deserialize_with = "optional_present")]
            time_zone_policy: Option<String>,
            #[serde(default, deserialize_with = "optional_present")]
            motion: Option<String>,
            #[serde(default, deserialize_with = "optional_present")]
            app_scale: Option<u16>,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("locale profile changes must be an object"));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        let changes = Self {
            display_language: wire.display_language,
            date_format: wire.date_format,
            number_format: wire.number_format,
            time_zone_policy: wire.time_zone_policy,
            motion: wire.motion,
            app_scale: wire.app_scale,
        };
        if changes.is_empty() {
            return Err(D::Error::custom("locale profile changes must not be empty"));
        }
        Ok(changes)
    }
}

/// Request body for `i18n.get_profile`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "Record<string, never>"))]
pub struct LocaleGetProfileRequest {}

impl<'de> Deserialize<'de> for LocaleGetProfileRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.as_object().is_some_and(serde_json::Map::is_empty) {
            Ok(Self {})
        } else {
            Err(D::Error::custom("expected an empty object"))
        }
    }
}

/// Request body for `i18n.set_profile`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleProfileSetRequest {
    /// Current profile revision required for this conditional update.
    pub expected_profile_revision: LocaleProfileRevision,
    /// Nonempty fields to update. Omitted fields are retained.
    pub changes: LocaleProfileChanges,
}

impl<'de> Deserialize<'de> for LocaleProfileSetRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            expected_profile_revision: LocaleProfileRevision,
            changes: LocaleProfileChanges,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom(
                "locale profile set request must be an object",
            ));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            expected_profile_revision: wire.expected_profile_revision,
            changes: wire.changes,
        })
    }
}

/// A profile field named by a `locale.changed` event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LocaleChangedField {
    /// The selected display language or its source changed.
    DisplayLanguage,
    /// The date format changed.
    DateFormat,
    /// The number format changed.
    NumberFormat,
    /// The time zone policy changed.
    TimeZonePolicy,
    /// The motion preference changed.
    Motion,
    /// The application scale changed.
    AppScale,
}

/// Body of a `locale.changed` event. The common event envelope supplies its
/// sequence and timestamp fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LocaleChangedBody {
    /// New canonical decimal profile snapshot revision.
    pub profile_revision: LocaleProfileRevision,
    /// Changed fields in declaration order, without duplicates.
    pub changed_fields: Vec<LocaleChangedField>,
    /// New selected registered language identifier.
    pub display_language: String,
}

impl<'de> Deserialize<'de> for LocaleChangedBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            profile_revision: LocaleProfileRevision,
            changed_fields: Vec<LocaleChangedField>,
            display_language: String,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if !value.is_object() {
            return Err(D::Error::custom("locale changed body must be an object"));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        if wire.changed_fields.is_empty()
            || wire
                .changed_fields
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(D::Error::custom(
                "locale changed fields must be unique and in declaration order",
            ));
        }
        Ok(Self {
            profile_revision: wire.profile_revision,
            changed_fields: wire.changed_fields,
            display_language: wire.display_language,
        })
    }
}

#[cfg(test)]
mod tests;
