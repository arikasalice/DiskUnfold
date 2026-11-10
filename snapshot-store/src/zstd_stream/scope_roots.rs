//! Extract native roots from the metadata of one verified, sealed spool.
use crate::records::NameEncoding;
use du_core_contracts::PathBytes;
use serde_json::Value;
mod unique;

/// Native scope roots in source order, without a display-string conversion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeRoots {
    encoding: NameEncoding,
    roots: Vec<Vec<u8>>,
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
}

pub(super) fn parse(metadata: &str) -> Result<ScopeRoots, ()> {
    let json: Value = serde_json::from_str(metadata).map_err(|_| ())?;
    unique::validate(metadata)?;
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
    Ok(ScopeRoots { encoding, roots })
}

pub(super) fn volume_descriptor(metadata: &str) -> Result<(), ()> {
    let json: Value = serde_json::from_str(metadata).map_err(|_| ())?;
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
