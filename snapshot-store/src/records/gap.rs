//! DUSNAP v1 Gap body. Roots and encoding must come from validated metadata.
use super::{MAX_FIELD_BYTES, NameEncoding, valid_name};
use du_core_contracts::Timestamp;

/// An unread range relative to a root retained in sealed snapshot metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gap<'a> {
    /// Zero-based index into the original scope roots.
    pub root_index: u32,
    /// Native path components. An empty path describes the root itself.
    pub path: Vec<&'a [u8]>,
    /// Machine-readable reason, including keys unknown to this reader.
    pub reason_key: &'a str,
    /// Observed UTC milliseconds, or none when no time was established.
    pub observed_at_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated,
    TrailingBytes,
    RootIndex,
    PathCount,
    Name,
    ReasonKey,
    Timestamp,
    RecordLength,
}

impl Gap<'_> {
    /// Encodes the body, excluding the frame length and record-kind byte.
    pub fn encode(&self, encoding: NameEncoding, root_count: usize) -> Result<Vec<u8>, Error> {
        if self.root_index as u64 >= root_count as u64 {
            return Err(Error::RootIndex);
        }
        let count = u16::try_from(self.path.len()).map_err(|_| Error::PathCount)?;
        if !valid_reason(self.reason_key.as_bytes()) {
            return Err(Error::ReasonKey);
        }
        let reason_len = u16::try_from(self.reason_key.len()).map_err(|_| Error::RecordLength)?;
        check_timestamp(self.observed_at_ms)?;
        let mut len = 4usize + 2 + 2 + self.reason_key.len() + 8;
        for name in &self.path {
            if !valid_component(encoding, name) {
                return Err(Error::Name);
            }
            len = len.checked_add(2 + name.len()).ok_or(Error::RecordLength)?;
        }
        if len as u64 + 1 > MAX_FIELD_BYTES {
            return Err(Error::RecordLength);
        }
        let mut out = Vec::with_capacity(len);
        out.extend(self.root_index.to_le_bytes());
        out.extend(count.to_le_bytes());
        for name in &self.path {
            out.extend((name.len() as u16).to_le_bytes());
            out.extend_from_slice(name);
        }
        out.extend(reason_len.to_le_bytes());
        out.extend_from_slice(self.reason_key.as_bytes());
        out.extend(self.observed_at_ms.unwrap_or(i64::MIN).to_le_bytes());
        Ok(out)
    }
}

/// Decodes one bounded body using the root count and encoding of the same snapshot.
pub fn decode(body: &[u8], encoding: NameEncoding, root_count: usize) -> Result<Gap<'_>, Error> {
    if body.len() as u64 + 1 > MAX_FIELD_BYTES {
        return Err(Error::RecordLength);
    }
    let mut input = body;
    let root_index = u32::from_le_bytes(array(&mut input)?);
    if root_index as u64 >= root_count as u64 {
        return Err(Error::RootIndex);
    }
    let count = u16::from_le_bytes(array(&mut input)?) as usize;
    let mut path = Vec::new();
    for _ in 0..count {
        let len = u16::from_le_bytes(array(&mut input)?) as usize;
        let name = take(&mut input, len)?;
        if !valid_component(encoding, name) {
            return Err(Error::Name);
        }
        path.push(name);
    }
    let len = u16::from_le_bytes(array(&mut input)?) as usize;
    let key = take(&mut input, len)?;
    if !valid_reason(key) {
        return Err(Error::ReasonKey);
    }
    let reason_key = std::str::from_utf8(key).map_err(|_| Error::ReasonKey)?;
    let timestamp = i64::from_le_bytes(array(&mut input)?);
    if !input.is_empty() {
        return Err(Error::TrailingBytes);
    }
    let observed_at_ms = (timestamp != i64::MIN).then_some(timestamp);
    check_timestamp(observed_at_ms)?;
    Ok(Gap {
        root_index,
        path,
        reason_key,
        observed_at_ms,
    })
}

fn check_timestamp(timestamp: Option<i64>) -> Result<(), Error> {
    if timestamp.is_some_and(|value| Timestamp::from_unix_millis(value).is_none()) {
        return Err(Error::Timestamp);
    }
    Ok(())
}

fn valid_component(encoding: NameEncoding, name: &[u8]) -> bool {
    valid_name(encoding, name)
        && match encoding {
            NameEncoding::Bytes => name != b"." && name != b"..",
            NameEncoding::WindowsUtf16Le => name != [b'.', 0] && name != [b'.', 0, b'.', 0],
        }
}
fn valid_reason(key: &[u8]) -> bool {
    !key.is_empty()
        && key
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
fn take<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8], Error> {
    if n > input.len() {
        return Err(Error::Truncated);
    }
    let (head, tail) = input.split_at(n);
    *input = tail;
    Ok(head)
}
fn array<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], Error> {
    take(input, N)?.try_into().map_err(|_| Error::Truncated)
}
#[cfg(test)]
mod tests;
