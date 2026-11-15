//! Compact in-memory forms of values that keep a decimal or UTC text wire form.
//!
//! Both types serialize to the same JSON strings as their earlier string-backed
//! forms; only the Rust representation is numeric.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, str::FromStr};

/// A byte count that travels as a canonical decimal string.
///
/// The wire form is digits only, no leading zero except for `"0"`, and within
/// the `u64` range, so JavaScript clients never round it through a double.
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub struct ByteSize(u64);

impl ByteSize {
    /// Parses the canonical decimal wire text.
    ///
    /// # Errors
    ///
    /// Returns an error for empty text, non-digit characters, a leading zero
    /// on a multi-digit number, or a value above `u64::MAX`.
    pub fn parse(value: &str) -> Result<Self, String> {
        let canonical = !value.is_empty()
            && value.bytes().all(|c| c.is_ascii_digit())
            && (value == "0" || !value.starts_with('0'));
        canonical
            .then(|| u64::from_str(value).ok())
            .flatten()
            .map(Self)
            .ok_or_else(|| "invalid ByteSize".to_owned())
    }

    /// Returns the byte count.
    pub fn get(&self) -> u64 {
        self.0
    }
}

impl From<u64> for ByteSize {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ByteSize").field(&self.to_string()).finish()
    }
}

impl Serialize for ByteSize {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ByteSize {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_str(TextVisitor(Self::parse, "a decimal byte count string"))
    }
}

/// An instant that travels as `YYYY-MM-DDTHH:MM:SS.mmmZ` in UTC.
///
/// The value holds Unix milliseconds. Only instants whose year has four digits
/// exist, so the text form always has 24 characters and formatting cannot fail.
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub struct Timestamp(i64);

/// Unix milliseconds of `0000-01-01T00:00:00.000Z`.
const MIN_MILLIS: i64 = -62_167_219_200_000;
/// Unix milliseconds of `9999-12-31T23:59:59.999Z`.
const MAX_MILLIS: i64 = 253_402_300_799_999;

impl Timestamp {
    /// Parses the fixed UTC wire text.
    ///
    /// The text must be exactly what [`Timestamp`] prints for the instant it
    /// names, so spellings such as a lowercase `t`, a space separator or a
    /// leap second (`:60`) are rejected.
    ///
    /// # Errors
    ///
    /// Returns an error when the text is not a valid canonical timestamp.
    pub fn parse(value: &str) -> Result<Self, String> {
        let shaped =
            value.len() == 24 && value.ends_with('Z') && value.as_bytes().get(19) == Some(&b'.');
        let millis = shaped
            .then(|| chrono::DateTime::parse_from_rfc3339(value).ok())
            .flatten()
            .map(|time| time.timestamp_millis())
            .and_then(Self::from_unix_millis)
            .filter(|time| time.to_string() == value);
        millis.ok_or_else(|| "invalid Timestamp".to_owned())
    }

    /// Encodes an injected epoch timestamp in the fixed UTC wire format.
    ///
    /// Returns `None` outside `0000-01-01T00:00:00.000Z` through
    /// `9999-12-31T23:59:59.999Z`, the instants the wire format can name.
    pub fn from_unix_millis(millis: i64) -> Option<Self> {
        (MIN_MILLIS..=MAX_MILLIS)
            .contains(&millis)
            .then_some(Self(millis))
    }

    /// Returns the instant as Unix milliseconds.
    pub fn unix_millis(&self) -> i64 {
        self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let days = self.0.div_euclid(86_400_000);
        let day_millis = self.0.rem_euclid(86_400_000);
        let (year, month, day) = civil_from_days(days);
        let (hour, minute) = (day_millis / 3_600_000, day_millis / 60_000 % 60);
        let (second, milli) = (day_millis / 1_000 % 60, day_millis % 1_000);
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milli:03}Z"
        )
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Timestamp").field(&self.to_string()).finish()
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_str(TextVisitor(Self::parse, "a UTC timestamp string"))
    }
}

/// Converts days since 1970-01-01 to a proleptic Gregorian year, month and day.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Accepts only JSON strings and hands their text to a validating parser.
struct TextVisitor<T>(fn(&str) -> Result<T, String>, &'static str);

impl<T> de::Visitor<'_> for TextVisitor<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.1)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<T, E> {
        (self.0)(value).map_err(E::custom)
    }
}
