//! Codec for the published v1 Item body. This does not validate a snapshot.
//!
//! Decode only after verifying the sealed container and framing. Stream order,
//! parent existence and terminal records remain the caller's responsibility.

use super::{MAX_FIELD_BYTES, MAX_IDENTITY_BYTES, NameEncoding, valid_name};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Folder,
    Link,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Exact(u64),
    LowerBound(u64),
    Unknown,
}

/// Flags and link count are preserved as raw values; their meanings are not
/// specified by the published wire table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item<'a> {
    pub number: u32,
    pub parent: u32,
    pub kind: Kind,
    pub flags: u16,
    pub name: &'a [u8],
    pub sizes: [Size; 3],
    pub modified_ms: Option<i64>,
    pub identity: &'a [u8],
    pub link_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated,
    TrailingBytes,
    InvalidKind,
    InvalidSizeState,
    InvalidNumber,
    InvalidName,
    IdentityLength,
    RecordLength,
    InvalidTimestamp,
}

impl Item<'_> {
    /// Encodes the body only; the framing length and kind are written separately.
    pub fn encode(&self, encoding: NameEncoding) -> Result<Vec<u8>, Error> {
        self.validate(encoding)?;
        let mut out = Vec::new();
        out.extend(self.number.to_le_bytes());
        out.extend(self.parent.to_le_bytes());
        out.push(match self.kind {
            Kind::File => 1,
            Kind::Folder => 2,
            Kind::Link => 3,
            Kind::Other => 4,
        });
        out.extend(self.flags.to_le_bytes());
        out.extend((self.name.len() as u16).to_le_bytes());
        out.extend(self.name);
        for size in self.sizes {
            match size {
                Size::Exact(value) => {
                    out.push(0);
                    out.extend(value.to_le_bytes());
                }
                Size::LowerBound(value) => {
                    out.push(1);
                    out.extend(value.to_le_bytes());
                }
                Size::Unknown => out.push(2),
            }
        }
        out.extend(self.modified_ms.unwrap_or(i64::MIN).to_le_bytes());
        out.push(self.identity.len() as u8);
        out.extend(self.identity);
        out.extend(self.link_count.to_le_bytes());
        if out.len() as u64 + 1 > MAX_FIELD_BYTES {
            return Err(Error::RecordLength);
        }
        Ok(out)
    }

    fn validate(&self, encoding: NameEncoding) -> Result<(), Error> {
        if self.number == 0 || self.parent >= self.number {
            return Err(Error::InvalidNumber);
        }
        if !valid_name(encoding, self.name) {
            return Err(Error::InvalidName);
        }
        if self.identity.len() > MAX_IDENTITY_BYTES {
            return Err(Error::IdentityLength);
        }
        if self.modified_ms == Some(i64::MIN) {
            return Err(Error::InvalidTimestamp);
        }
        Ok(())
    }
}

/// Parses one complete Item body without copying its native name or identity.
pub fn decode(body: &[u8], encoding: NameEncoding) -> Result<Item<'_>, Error> {
    if body.len() as u64 + 1 > MAX_FIELD_BYTES {
        return Err(Error::RecordLength);
    }
    let mut input = body;
    let number = u32::from_le_bytes(take_array(&mut input)?);
    let parent = u32::from_le_bytes(take_array(&mut input)?);
    let kind = match take_array::<1>(&mut input)?[0] {
        1 => Kind::File,
        2 => Kind::Folder,
        3 => Kind::Link,
        4 => Kind::Other,
        _ => return Err(Error::InvalidKind),
    };
    let flags = u16::from_le_bytes(take_array(&mut input)?);
    let name_len = u16::from_le_bytes(take_array(&mut input)?) as usize;
    let name = take(&mut input, name_len)?;
    let sizes = [
        read_size(&mut input)?,
        read_size(&mut input)?,
        read_size(&mut input)?,
    ];
    let timestamp = i64::from_le_bytes(take_array(&mut input)?);
    let identity_len = take_array::<1>(&mut input)?[0] as usize;
    let identity = take(&mut input, identity_len)?;
    let link_count = u32::from_le_bytes(take_array(&mut input)?);
    if !input.is_empty() {
        return Err(Error::TrailingBytes);
    }
    let item = Item {
        number,
        parent,
        kind,
        flags,
        name,
        sizes,
        modified_ms: (timestamp != i64::MIN).then_some(timestamp),
        identity,
        link_count,
    };
    item.validate(encoding)?;
    Ok(item)
}

fn read_size(input: &mut &[u8]) -> Result<Size, Error> {
    match take_array::<1>(input)?[0] {
        0 => Ok(Size::Exact(u64::from_le_bytes(take_array(input)?))),
        1 => Ok(Size::LowerBound(u64::from_le_bytes(take_array(input)?))),
        2 => Ok(Size::Unknown),
        _ => Err(Error::InvalidSizeState),
    }
}

fn take<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8], Error> {
    if input.len() < n {
        return Err(Error::Truncated);
    }
    let (head, tail) = input.split_at(n);
    *input = tail;
    Ok(head)
}

fn take_array<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], Error> {
    take(input, N)?.try_into().map_err(|_| Error::Truncated)
}

#[cfg(test)]
mod tests;
