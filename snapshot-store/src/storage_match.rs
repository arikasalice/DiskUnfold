//! F08 storage correspondence for two independently observed identities.
//!
//! This pure decision does not discover or validate OS identities. Its inputs
//! must come from the relevant validated observations, and names, mount paths,
//! and drive letters are not identity evidence.

/// Published storage correspondence strategy identifier.
pub const RULE_ID: &str = "compare.storage-match";
/// Published storage correspondence strategy revision.
pub const RULE_REVISION: u32 = 1;

/// The capability recorded at observation time, not a current OS guess.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability {
    /// The identity capability was supported.
    Supported,
    /// The capability was only partially supported.
    Partial,
    /// The capability was unavailable, unsupported, or not known.
    Unavailable,
}

/// Identity evidence from a single validated observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation<'a> {
    /// Recorded identity capability.
    pub capability: Capability,
    /// Recorded identity kind, if independently available.
    pub kind: Option<&'a [u8]>,
    /// Recorded identity value, if independently available.
    pub value: Option<&'a [u8]>,
}

/// Result of the published pairwise storage correspondence table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Correspondence {
    /// Both supported observations contain the same kind and value.
    Same,
    /// Both supported observations contain the same kind but different values.
    Different,
    /// Identity is incomplete, capabilities are insufficient, or kinds differ.
    Unverifiable,
}

/// Compare two recorded identity observations without interpreting their names.
///
/// Empty or absent identity fields are unverified. Equal pairwise identity
/// does not establish uniqueness, strength, current access, or authorization;
/// F19 must determine those separately from its own evidence.
#[must_use]
pub fn compare(before: Observation<'_>, after: Observation<'_>) -> Correspondence {
    if before.capability != Capability::Supported || after.capability != Capability::Supported {
        return Correspondence::Unverifiable;
    }
    match (before.kind, before.value, after.kind, after.value) {
        (Some(bk), Some(bv), Some(ak), Some(av))
            if !bk.is_empty() && !bv.is_empty() && !ak.is_empty() && !av.is_empty() =>
        {
            if bk != ak {
                Correspondence::Unverifiable
            } else if bv == av {
                Correspondence::Same
            } else {
                Correspondence::Different
            }
        }
        _ => Correspondence::Unverifiable,
    }
}

#[cfg(test)]
mod tests;
