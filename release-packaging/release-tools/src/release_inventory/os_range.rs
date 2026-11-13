use serde::{Deserialize, Serialize, de};
use std::{error::Error, fmt};

/// The operating-system family supported by a distribution row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OsFamily {
    /// Microsoft Windows.
    Windows,
    /// Apple macOS.
    Macos,
    /// Ubuntu Linux.
    Ubuntu,
}

impl OsFamily {
    pub(super) fn sort_key(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Ubuntu => "ubuntu",
            Self::Windows => "windows",
        }
    }
}

/// An OS release label ordered by its numeric components.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OsVersion(String);

impl OsVersion {
    /// Parses an OS release label such as `11 25H2`, `15`, or `24.04`.
    pub fn parse(value: &str) -> Result<Self, OsVersionParseError> {
        if value.trim().is_empty() || value.chars().any(char::is_control) {
            return Err(OsVersionParseError::new(
                "must be nonempty and contain no control characters",
            ));
        }
        let numeric_groups = numeric_groups(value)?;
        if numeric_groups.is_empty() {
            return Err(OsVersionParseError::new(
                "must contain at least one numeric release component",
            ));
        }
        Ok(Self(value.trim().to_owned()))
    }
}

impl fmt::Display for OsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Ord for OsVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        numeric_groups(&self.0)
            .unwrap_or_default()
            .cmp(&numeric_groups(&other.0).unwrap_or_default())
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for OsVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<'de> Deserialize<'de> for OsVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

fn numeric_groups(value: &str) -> Result<Vec<u32>, OsVersionParseError> {
    let mut groups = Vec::new();
    let mut number = 0_u32;
    let mut in_number = false;
    for character in value.chars() {
        if let Some(digit) = character.to_digit(10).filter(|_| character.is_ascii()) {
            number = number
                .checked_mul(10)
                .and_then(|current| current.checked_add(digit))
                .ok_or_else(|| {
                    OsVersionParseError::new("numeric release component is too large")
                })?;
            in_number = true;
        } else if in_number {
            groups.push(number);
            number = 0;
            in_number = false;
        }
    }
    if in_number {
        groups.push(number);
    }
    Ok(groups)
}

/// A malformed or unorderable OS version label.
#[derive(Debug)]
pub struct OsVersionParseError {
    detail: String,
}

impl OsVersionParseError {
    fn new(detail: &str) -> Self {
        Self {
            detail: detail.to_owned(),
        }
    }
}

impl fmt::Display for OsVersionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid os_version: {}", self.detail)
    }
}

impl Error for OsVersionParseError {}

/// An OS family paired with a supported version range or an exact version set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OsRange {
    /// OS family for this distribution row.
    pub family: OsFamily,
    /// Version scope. Exact sets keep non-consecutive supported releases explicit.
    pub versions: OsVersionScope,
}

/// The version scope supported for one OS family.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum OsVersionScope {
    /// Every version from `minimum` through `maximum`, inclusive. `None` is unbounded above.
    Range {
        /// Lowest supported version, inclusive.
        minimum: OsVersion,
        /// Highest supported version, inclusive, when bounded.
        maximum: Option<OsVersion>,
    },
    /// Only these explicitly listed versions are supported.
    Exact {
        /// Supported versions. The inventory validator sorts this list for stable output.
        supported_versions: Vec<OsVersion>,
    },
}

impl Serialize for OsRange {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        #[serde(untagged)]
        enum OsRangeRef<'a> {
            Range {
                family: OsFamily,
                minimum: &'a OsVersion,
                maximum: &'a Option<OsVersion>,
            },
            Exact {
                family: OsFamily,
                supported_versions: &'a [OsVersion],
            },
        }

        let value = match &self.versions {
            OsVersionScope::Range { minimum, maximum } => OsRangeRef::Range {
                family: self.family,
                minimum,
                maximum,
            },
            OsVersionScope::Exact { supported_versions } => OsRangeRef::Exact {
                family: self.family,
                supported_versions,
            },
        };
        value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OsRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct OsRangeInput {
            family: OsFamily,
            #[serde(default, deserialize_with = "deserialize_present_option")]
            minimum: Present<OsVersion>,
            #[serde(default, deserialize_with = "deserialize_present_option")]
            maximum: Present<OsVersion>,
            #[serde(default, deserialize_with = "deserialize_present_option")]
            supported_versions: Present<Vec<OsVersion>>,
        }

        let input = OsRangeInput::deserialize(deserializer)?;
        let range_field_present = input.minimum.present || input.maximum.present;
        let versions = match (input.supported_versions.present, range_field_present) {
            (true, true) => {
                return Err(de::Error::custom(
                    "supported_versions cannot be combined with minimum or maximum",
                ));
            }
            (true, false) => {
                let Some(supported_versions) = input.supported_versions.value else {
                    return Err(de::Error::custom("supported_versions must be an array"));
                };
                OsVersionScope::Exact { supported_versions }
            }
            (false, _) => {
                let Some(minimum) = input.minimum.value else {
                    return Err(de::Error::custom(
                        "minimum is required when supported_versions is absent",
                    ));
                };
                OsVersionScope::Range {
                    minimum,
                    maximum: input.maximum.value,
                }
            }
        };

        Ok(Self {
            family: input.family,
            versions,
        })
    }
}

struct Present<T> {
    present: bool,
    value: Option<T>,
}

impl<T> Default for Present<T> {
    fn default() -> Self {
        Self {
            present: false,
            value: None,
        }
    }
}

fn deserialize_present_option<'de, D, T>(deserializer: D) -> Result<Present<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(|value| Present {
        present: true,
        value,
    })
}
