use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
use std::{fmt, str::FromStr};

macro_rules! string_value {
    ($name:ident, $validate:expr) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        #[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
        #[cfg_attr(feature = "typescript", ts(type = "string"))]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: &str) -> Result<Self, String> {
                if ($validate)(value) {
                    Ok(Self(value.to_owned()))
                } else {
                    Err(format!("invalid {}", stringify!($name)))
                }
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.0)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::parse(&String::deserialize(d)?).map_err(D::Error::custom)
            }
        }
    };
}
string_value!(Id, |s: &str| uuid::Uuid::parse_str(s).is_ok_and(|v| v
    .get_version_num()
    == 7
    && v.get_variant() == uuid::Variant::RFC4122
    && v.to_string() == s));
impl Id {
    /// Builds a UUID v7 from bytes with a supplied timestamp and random data.
    pub fn from_v7_bytes(bytes: [u8; 16]) -> Self {
        Self(
            uuid::Builder::from_bytes(bytes)
                .with_version(uuid::Version::SortRand)
                .with_variant(uuid::Variant::RFC4122)
                .into_uuid()
                .to_string(),
        )
    }
}
string_value!(ByteSize, |s: &str| !s.is_empty()
    && s.bytes().all(|c| c.is_ascii_digit())
    && (s == "0" || !s.starts_with('0'))
    && u64::from_str(s).is_ok());
string_value!(Timestamp, |s: &str| s.len() == 24
    && s.ends_with('Z')
    && s.as_bytes().get(19) == Some(&b'.')
    && chrono::DateTime::parse_from_rfc3339(s).is_ok());
impl Timestamp {
    /// Encodes an injected epoch timestamp in the fixed UTC wire format.
    pub fn from_unix_millis(millis: i64) -> Option<Self> {
        let time = chrono::DateTime::from_timestamp_millis(millis)?;
        Self::parse(&time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).ok()
    }
}
impl From<u64> for ByteSize {
    fn from(value: u64) -> Self {
        Self(value.to_string())
    }
}
impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SizeBasis {
    Logical,
    Allocated,
    UniquePhysical,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SizeEvidence {
    Measured,
    OsReported,
    Estimated,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum SizeAmount {
    Exact { bytes: ByteSize },
    LowerBound { bytes: ByteSize },
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SizeValue {
    pub basis: SizeBasis,
    pub evidence: SizeEvidence,
    #[serde(flatten)]
    pub amount: SizeAmount,
}

impl<'de> Deserialize<'de> for SizeValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            basis: SizeBasis,
            evidence: SizeEvidence,
            state: String,
            bytes: Option<ByteSize>,
        }
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.get("state").and_then(|v| v.as_str()) == Some("unknown")
            && value.get("bytes").is_some()
        {
            return Err(D::Error::custom("unknown size cannot contain bytes"));
        }
        let wire: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        let amount = match (wire.state.as_str(), wire.bytes) {
            ("exact", Some(bytes)) => SizeAmount::Exact { bytes },
            ("lower_bound", Some(bytes)) => SizeAmount::LowerBound { bytes },
            ("unknown", None) => SizeAmount::Unknown,
            _ => return Err(D::Error::custom("invalid size state or bytes")),
        };
        Ok(Self {
            basis: wire.basis,
            evidence: wire.evidence,
            amount,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct OsPath {
    pub encoding: PathEncoding,
    pub raw: PathBytes,
    pub display: String,
    pub lossy: bool,
}
impl PartialEq for OsPath {
    fn eq(&self, other: &Self) -> bool {
        self.encoding == other.encoding && self.raw == other.raw
    }
}
impl Eq for OsPath {}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PathEncoding {
    Unix,
    WindowsUtf16le,
}
string_value!(PathBytes, |s: &str| URL_SAFE_NO_PAD
    .decode(s)
    .is_ok_and(|b| URL_SAFE_NO_PAD.encode(b) == s));
impl PathBytes {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }
    pub fn bytes(&self) -> Result<Vec<u8>, base64::DecodeError> {
        URL_SAFE_NO_PAD.decode(&self.0)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ItemRef {
    pub result_version: Id,
    pub item_no: u32,
}
