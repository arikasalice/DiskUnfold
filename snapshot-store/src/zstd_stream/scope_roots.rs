//! Extract native roots from the metadata of one verified, sealed spool.
use crate::records::NameEncoding;
use du_core_contracts::PathBytes;
use serde_json::Value;
mod source_metadata;
mod unique;

/// A parsed path separator from one validated path rule descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathSeparator {
    /// Unix-style forward slash.
    Slash,
    /// Windows-style backslash.
    Backslash,
}

/// A validated case-sensitivity value from one path rule descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathCaseSensitivity {
    /// Case-sensitive path comparison.
    Sensitive,
    /// Case-insensitive path comparison.
    Insensitive,
}

/// A validated Unicode normalization value from one path rule descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathNormalization {
    /// No Unicode normalization.
    None,
    /// NFC normalization.
    Nfc,
    /// NFD normalization.
    Nfd,
}

/// One original source path-rule descriptor with its validated known fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRuleDescriptor {
    original: Value,
    separator: PathSeparator,
    case_sensitivity: Option<PathCaseSensitivity>,
    normalization: Option<PathNormalization>,
}

impl PathRuleDescriptor {
    /// Returns the unchanged descriptor value parsed from the sealed metadata.
    pub fn original(&self) -> &Value {
        &self.original
    }

    /// Returns the validated separator.
    pub fn separator(&self) -> PathSeparator {
        self.separator
    }

    /// Returns the known sensitivity, or none when the source recorded it as unknown.
    pub fn case_sensitivity(&self) -> Option<PathCaseSensitivity> {
        self.case_sensitivity
    }

    /// Returns the known normalization, or none when the source recorded it as unknown.
    pub fn normalization(&self) -> Option<PathNormalization> {
        self.normalization
    }

    fn parse(value: &Value, expected_separator: &str) -> Result<Self, ()> {
        let object = value.as_object().ok_or(())?;
        let separator = match object.get("separator").and_then(Value::as_str) {
            Some("/") if expected_separator == "/" => PathSeparator::Slash,
            Some("\\") if expected_separator == "\\" => PathSeparator::Backslash,
            _ => return Err(()),
        };
        let case_sensitivity = match object.get("caseSensitivity") {
            Some(Value::Null) => None,
            Some(Value::String(value)) if value == "sensitive" => {
                Some(PathCaseSensitivity::Sensitive)
            }
            Some(Value::String(value)) if value == "insensitive" => {
                Some(PathCaseSensitivity::Insensitive)
            }
            _ => return Err(()),
        };
        let normalization = match object.get("normalization") {
            Some(Value::Null) => None,
            Some(Value::String(value)) if value == "none" => Some(PathNormalization::None),
            Some(Value::String(value)) if value == "NFC" => Some(PathNormalization::Nfc),
            Some(Value::String(value)) if value == "NFD" => Some(PathNormalization::Nfd),
            _ => return Err(()),
        };
        Ok(Self {
            original: value.clone(),
            separator,
            case_sensitivity,
            normalization,
        })
    }
}

/// Native scope roots and independently ordered metadata from one sealed snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeRoots {
    encoding: NameEncoding,
    roots: Vec<Vec<u8>>,
    path_rules: Option<Vec<PathRuleDescriptor>>,
    volume_record: Option<super::ValidatedVolumeRecord>,
}
impl ScopeRoots {
    /// The original path encoding.
    pub fn encoding(&self) -> NameEncoding {
        self.encoding
    }
    /// Original absolute path bytes, in the source snapshot's root order.
    pub fn roots(&self) -> &[Vec<u8>] {
        &self.roots
    }

    /// Original path rules in source order, only when stable source metadata passed validation.
    /// The rule list remains independent of the root list; matching lengths do not bind indices.
    pub fn path_rules(&self) -> Option<&[PathRuleDescriptor]> {
        self.path_rules.as_deref()
    }

    /// The same sealed file's validated Volume record, when Volume validation ran.
    pub fn volume_record(&self) -> Option<&super::ValidatedVolumeRecord> {
        self.volume_record.as_ref()
    }

    pub(super) fn set_validated_volume_record(
        &mut self,
        volume_record: super::ValidatedVolumeRecord,
    ) {
        self.volume_record = Some(volume_record);
    }
}

/// Validates roots and, when required, stable source metadata from one parsed object.
pub(super) fn parse(metadata: &str, require_volume: bool) -> Result<ScopeRoots, ()> {
    let json: Value = serde_json::from_str(metadata).map_err(|_| ())?;
    unique::validate(metadata)?;
    if require_volume {
        volume_descriptor(&json)?;
        stable_metadata(&json)?;
    }
    let encoding = match json.get("pathEncoding").and_then(Value::as_str) {
        Some("bytes") => NameEncoding::Bytes,
        Some("utf16le") => NameEncoding::WindowsUtf16Le,
        _ => return Err(()),
    };
    let entries = json
        .get("source")
        .and_then(|v| v.get("scopeRoots"))
        .and_then(Value::as_array)
        .ok_or(())?;
    let path_rules = require_volume
        .then(|| source_metadata::validate(&json, encoding, entries.len()))
        .transpose()?;
    let mut roots = Vec::with_capacity(entries.len());
    for entry in entries {
        let encoded = entry.as_str().ok_or(())?;
        let bytes = PathBytes::parse(encoded)
            .map_err(|_| ())?
            .bytes()
            .map_err(|_| ())?;
        if !absolute(encoding, &bytes) {
            return Err(());
        }
        roots.push(bytes);
    }
    Ok(ScopeRoots {
        encoding,
        roots,
        path_rules,
        volume_record: None,
    })
}

// These checks cover the published identity and observation fields, not the
// still-separate condition and publication contracts.
fn stable_metadata(json: &Value) -> Result<(), ()> {
    let object = json.as_object().ok_or(())?;
    if object.get("formatId").and_then(Value::as_str) != Some("dusnap")
        || object.get("formatVersion").and_then(Value::as_u64) != Some(1)
        || !object
            .get("appVersion")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty())
        || !valid_id(object.get("snapshotId"))
        || !valid_time(object.get("createdAt"))
        || object.get("label").is_some_and(|v| !v.is_string())
    {
        return Err(());
    }
    let source = object.get("source").and_then(Value::as_object).ok_or(())?;
    if !valid_id(source.get("resultVersionId"))
        || !nullable(source.get("scopeVersionId"), valid_id)
        || !nullable(source.get("lineageId"), valid_id)
        || !nullable(source.get("observedAt"), valid_time)
    {
        return Err(());
    }
    Ok(())
}

fn valid_id(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|v| du_core_contracts::Id::parse(v).is_ok())
}

fn valid_time(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|v| du_core_contracts::Timestamp::parse(v).is_ok())
}

fn nullable(value: Option<&Value>, valid: fn(Option<&Value>) -> bool) -> bool {
    value.is_some_and(|v| v.is_null() || valid(Some(v)))
}

fn volume_descriptor(json: &Value) -> Result<(), ()> {
    let descriptor = json.get("volume").and_then(Value::as_object).ok_or(())?;
    if descriptor.get("recordKind").and_then(Value::as_u64) != Some(1) {
        return Err(());
    }
    Ok(())
}

fn absolute(encoding: NameEncoding, bytes: &[u8]) -> bool {
    match encoding {
        NameEncoding::Bytes => {
            bytes.first() == Some(&b'/')
                && !bytes.contains(&0)
                && bytes.split(|b| *b == b'/').all(|p| p != b"." && p != b"..")
        }
        NameEncoding::WindowsUtf16Le => windows_absolute(bytes),
    }
}
fn windows_absolute(bytes: &[u8]) -> bool {
    const SEPARATOR: u16 = b'\\' as u16;
    const COLON: u16 = b':' as u16;
    const DOT: u16 = b'.' as u16;
    const UNC: [u16; 2] = [SEPARATOR, SEPARATOR];
    const VERBATIM: [u16; 4] = [SEPARATOR, SEPARATOR, b'?' as u16, SEPARATOR];
    const DEVICE: [u16; 4] = [SEPARATOR, SEPARATOR, DOT, SEPARATOR];
    let (units, tail) = bytes.as_chunks::<2>();
    if !tail.is_empty() || units.iter().any(|u| *u == [0, 0] || *u == [b'/', 0]) {
        return false;
    }
    let decoded: Vec<u16> = units.iter().map(|u| u16::from_le_bytes(*u)).collect();
    // Accept file namespace roots, without interpreting or opening devices.
    if decoded.starts_with(&DEVICE) {
        return false;
    }
    let extended = decoded.strip_prefix(&VERBATIM);
    let body = extended.unwrap_or(&decoded);
    let tail = match body {
        [letter, COLON, SEPARATOR, tail @ ..]
            if u8::try_from(*letter).is_ok_and(|letter| letter.is_ascii_alphabetic()) =>
        {
            tail
        }
        _ if extended.is_some() && has_prefix(body, b"UNC\\") => {
            let Some(tail) = unc_tail(&body[4..], SEPARATOR) else {
                return false;
            };
            tail
        }
        _ if extended.is_some() && volume_guid_root(body, SEPARATOR) => &body[45..],
        _ if extended.is_none() && body.starts_with(&UNC) => {
            let Some(tail) = unc_tail(&body[2..], SEPARATOR) else {
                return false;
            };
            tail
        }
        _ => return false,
    };
    tail.split(|unit| *unit == SEPARATOR)
        .all(|part| part != [DOT] && part != [DOT, DOT] && !part.contains(&COLON))
}

fn unc_tail(path: &[u16], separator: u16) -> Option<&[u16]> {
    let mut parts = path.splitn(3, |unit| *unit == separator);
    for component in [parts.next()?, parts.next()?] {
        if component.is_empty()
            || component == [b'.' as u16]
            || component == [b'.' as u16; 2]
            || component.contains(&(b':' as u16))
        {
            return None;
        }
    }
    Some(parts.next().unwrap_or_default())
}

fn has_prefix(path: &[u16], ascii: &[u8]) -> bool {
    path.get(..ascii.len()).is_some_and(|prefix| {
        prefix.iter().zip(ascii).all(|(&unit, byte)| {
            u8::try_from(unit).is_ok_and(|unit| unit.eq_ignore_ascii_case(byte))
        })
    })
}

fn volume_guid_root(path: &[u16], separator: u16) -> bool {
    // A volume GUID prefix is `Volume{8-4-4-4-12}\`, including its separator.
    has_prefix(path, b"Volume{")
        && path.get(43..45) == Some(&[b'}' as u16, separator])
        && path[7..43].iter().enumerate().all(|(position, &unit)| {
            if matches!(position, 8 | 13 | 18 | 23) {
                unit == b'-' as u16
            } else {
                u8::try_from(unit).is_ok_and(|unit| unit.is_ascii_hexdigit())
            }
        })
}
