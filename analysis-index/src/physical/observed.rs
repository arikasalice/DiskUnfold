//! What one file's space evidence contributes to a physical total.

use super::{FileIdentity, Known, SizeAmount, SizeBasis, SizeEvidence, SpaceObservation};

/// A file's reading, independent of the identity table that will hold it.
pub(super) enum Observed<'a> {
    /// No usable allocation: absent, zero links, or not an OS-reported allocation.
    Missing,
    /// A usable allocation without a non-empty volume and file identity.
    IdentityMissing,
    /// A usable allocation of the identified file.
    Known(&'a FileIdentity, Known),
}

impl<'a> Observed<'a> {
    /// Classifies one file's evidence without copying it.
    pub(super) fn read(input: Option<&'a SpaceObservation>) -> Self {
        let Some(input) = input else {
            return Self::Missing;
        };
        if input.link_count == Some(0) {
            return Self::Missing;
        }
        let value = input
            .allocated
            .as_ref()
            .and_then(|v| {
                if v.basis != SizeBasis::Allocated || v.evidence != SizeEvidence::OsReported {
                    return None;
                }
                match &v.amount {
                    SizeAmount::Exact { bytes } => Some((bytes, false)),
                    SizeAmount::LowerBound { bytes } => Some((bytes, true)),
                    SizeAmount::Unknown => None,
                }
            })
            .map(|(bytes, lower)| Known {
                bytes: bytes.get(),
                lower,
                sharing: input.sharing,
            });
        let Some(value) = value else {
            return Self::Missing;
        };
        match &input.identity {
            Some(identity) if !identity.volume.is_empty() && !identity.file.is_empty() => {
                Self::Known(identity, value)
            }
            _ => Self::IdentityMissing,
        }
    }
}
