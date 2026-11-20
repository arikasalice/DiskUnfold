//! Canonical decimal ratio used by visualization rows.

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Canonical signed decimal ratio, with at most 20 fractional digits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS), ts(type = "string"))]
#[serde(transparent)]
pub struct VizRatio(String);
impl VizRatio {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        let unsigned = value.strip_prefix('-').unwrap_or(value);
        let mut parts = unsigned.split('.');
        let integer = parts.next().unwrap_or("");
        let fractional = parts.next();
        let valid_integer = !integer.is_empty()
            && integer.bytes().all(|b| b.is_ascii_digit())
            && (integer == "0" || !integer.starts_with('0'));
        let valid_fraction = fractional.is_none_or(|f| {
            !f.is_empty()
                && f.len() <= 20
                && f.bytes().all(|b| b.is_ascii_digit())
                && !f.ends_with('0')
        });
        if !valid_integer || !valid_fraction || parts.next().is_some() || value == "-0" {
            return Err("invalid canonical ratio");
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for VizRatio {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(D::Error::custom)
    }
}
