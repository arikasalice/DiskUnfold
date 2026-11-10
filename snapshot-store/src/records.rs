//! Streaming validation of the published v1 record invariants.
//!
//! This module does not encode files or infer missing snapshot metadata.

pub mod item;

/// Maximum metadata and declared record length in bytes.
pub const MAX_FIELD_BYTES: u64 = 65_536;
/// Maximum decoded record stream size, in bytes.
pub const MAX_DECODED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Maximum native name length, in bytes.
pub const MAX_NAME_BYTES: usize = 1_024;
/// Maximum observed item identity length, in bytes.
pub const MAX_IDENTITY_BYTES: usize = 32;

/// Encoding of native name units from the observed result, without normalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameEncoding {
    WindowsUtf16Le,
    Bytes,
}

/// The structural fields needed to validate one record; data remains borrowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record<'a> {
    Volume,
    Item {
        number: u32,
        parent: u32,
        name: &'a [u8],
        identity: &'a [u8],
    },
    Gap {
        item: u32,
        reason_key: &'a str,
    },
    End,
}

/// The failed invariant. Validation never changes state on failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    MetadataLength,
    RecordLength,
    DecodedLimit,
    RecordOrder,
    ItemNumber,
    Parent,
    Name,
    IdentityLength,
    GapItem,
    MissingEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Volume,
    Items,
    Gaps,
    End,
}

/// Checks a v1 record stream in order, without buffering or changing its data.
///
/// It does not establish path ordering, validate codec framing or preserve
/// metadata; those require a pinned source and the complete v1 codec.
pub struct Validator {
    encoding: NameEncoding,
    phase: Phase,
    last_item: u32,
    decoded_bytes: u64,
}

impl Validator {
    /// Creates a validator after checking the published metadata length bounds.
    pub fn new(encoding: NameEncoding, metadata_bytes: u64) -> Result<Self, ValidationError> {
        if !(1..=MAX_FIELD_BYTES).contains(&metadata_bytes) {
            return Err(ValidationError::MetadataLength);
        }
        Ok(Self {
            encoding,
            phase: Phase::Volume,
            last_item: 0,
            decoded_bytes: 0,
        })
    }

    /// Checks one record and advances only if all its invariants hold.
    ///
    /// The caller supplies the declared record length and the actual decoded
    /// byte count consumed by the record, including framing. These lengths
    /// must be measured by the codec; this method does not parse byte frames.
    pub fn accept(
        &mut self,
        record: Record<'_>,
        declared_length: u64,
        decoded_bytes: u64,
    ) -> Result<(), ValidationError> {
        if declared_length > MAX_FIELD_BYTES {
            return Err(ValidationError::RecordLength);
        }
        let total = self
            .decoded_bytes
            .checked_add(decoded_bytes)
            .filter(|n| *n <= MAX_DECODED_BYTES)
            .ok_or(ValidationError::DecodedLimit)?;
        let (phase, last_item) = self.check(record)?;
        self.phase = phase;
        self.last_item = last_item;
        self.decoded_bytes = total;
        Ok(())
    }

    fn check(&self, record: Record<'_>) -> Result<(Phase, u32), ValidationError> {
        match (self.phase, record) {
            (Phase::Volume, Record::Volume) => Ok((Phase::Items, 0)),
            (
                Phase::Items,
                Record::Item {
                    number,
                    parent,
                    name,
                    identity,
                },
            ) => {
                if self.last_item.checked_add(1) != Some(number) {
                    return Err(ValidationError::ItemNumber);
                }
                if parent >= number {
                    return Err(ValidationError::Parent);
                }
                if !valid_name(self.encoding, name) {
                    return Err(ValidationError::Name);
                }
                if identity.len() > MAX_IDENTITY_BYTES {
                    return Err(ValidationError::IdentityLength);
                }
                Ok((Phase::Items, number))
            }
            (Phase::Items | Phase::Gaps, Record::Gap { item, .. }) => {
                // Item zero is the implicit scope root used by parent references.
                if item > self.last_item {
                    return Err(ValidationError::GapItem);
                }
                Ok((Phase::Gaps, self.last_item))
            }
            (Phase::Items | Phase::Gaps, Record::End) => Ok((Phase::End, self.last_item)),
            _ => Err(ValidationError::RecordOrder),
        }
    }

    /// Requires the single terminal End record before accepting the stream.
    pub fn finish(self) -> Result<(), ValidationError> {
        if self.phase == Phase::End {
            Ok(())
        } else {
            Err(ValidationError::MissingEnd)
        }
    }
}

fn valid_name(encoding: NameEncoding, name: &[u8]) -> bool {
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return false;
    }
    match encoding {
        NameEncoding::Bytes => !name.iter().any(|byte| matches!(byte, 0 | b'/')),
        NameEncoding::WindowsUtf16Le => {
            let (pairs, remainder) = name.as_chunks::<2>();
            remainder.is_empty()
                && pairs
                    .iter()
                    .all(|pair| !matches!(u16::from_le_bytes(*pair), 0 | 47 | 92))
        }
    }
}

#[cfg(test)]
mod tests;
