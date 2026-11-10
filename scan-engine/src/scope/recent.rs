//! F01 recent-scope correspondence over independently verified observations.
//!
//! This module performs no enumeration, path normalization, or persistence.
//! A caller must obtain the identities and entry comparison from their owners.

/// Completeness of current volume enumeration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Inventory {
    /// The provider confirmed the list is complete.
    Complete,
    /// Some volumes may be missing.
    Partial,
    /// Enumeration was unavailable or failed.
    Unavailable,
}

/// Identity evidence, already classified by its owning provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Identity<'a> {
    /// A nonempty, independently confirmed volume identity.
    Confirmed { kind: &'a [u8], value: &'a [u8] },
    /// Identity is unavailable, unsupported, or not confirmed.
    Unknown,
}

/// A provider's comparison of a current entry with the saved entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryComparison {
    /// The entry matches the saved entry under verified comparison rules.
    Same,
    /// The entry is confirmed to differ from the saved entry.
    Different,
    /// Comparison could not be confirmed.
    Unknown,
}

/// One currently observed volume and its independently checked entry path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurrentVolume<'a> {
    /// Provider-verified volume identity.
    pub identity: Identity<'a>,
    /// Raw candidate path. It is returned only as a proposal, never applied.
    pub entry_path: &'a [u8],
    /// Provider-verified comparison with the saved entry.
    pub entry_comparison: EntryComparison,
}

/// Correspondence to a saved scope target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Correspondence<'a> {
    /// The same volume is present at the verified same entry.
    Matched,
    /// The same volume has one confirmed different entry.
    Moved { proposed_entry: &'a [u8] },
    /// A complete, conclusive inventory contains no matching volume.
    Disconnected,
    /// The available observations cannot support a determination.
    Unverifiable,
}

/// Applies the F01 correspondence table without changing the saved scope.
///
/// Unknown identities, incomplete enumeration, or multiple same-ID entries
/// cannot establish an absent or uniquely moved volume. A confirmed same entry
/// remains usable with a partial list when no conflicting observation exists.
#[must_use]
pub fn correspond<'a>(
    saved: Identity<'_>,
    inventory: Inventory,
    current: &'a [CurrentVolume<'a>],
) -> Correspondence<'a> {
    let Identity::Confirmed { kind, value: saved } = saved else {
        return Correspondence::Unverifiable;
    };
    if kind.is_empty() || saved.is_empty() || inventory == Inventory::Unavailable {
        return Correspondence::Unverifiable;
    }
    let mut same = None;
    let mut unknown = false;
    for volume in current {
        match volume.identity {
            Identity::Confirmed {
                kind: current_kind,
                value: id,
            } if !current_kind.is_empty()
                && !id.is_empty()
                && current_kind == kind
                && id == saved =>
            {
                if same.replace(volume).is_some() {
                    return Correspondence::Unverifiable;
                }
            }
            Identity::Confirmed {
                kind: current_kind,
                value: id,
            } if !current_kind.is_empty() && !id.is_empty() && current_kind == kind => {}
            _ => unknown = true,
        }
    }
    match same {
        Some(volume)
            if !unknown
                && !volume.entry_path.is_empty()
                && volume.entry_comparison == EntryComparison::Same =>
        {
            Correspondence::Matched
        }
        Some(volume)
            if !unknown
                && inventory == Inventory::Complete
                && volume.entry_comparison == EntryComparison::Different
                && !volume.entry_path.is_empty() =>
        {
            Correspondence::Moved {
                proposed_entry: volume.entry_path,
            }
        }
        None if !unknown && inventory == Inventory::Complete => Correspondence::Disconnected,
        _ => Correspondence::Unverifiable,
    }
}

#[cfg(test)]
mod tests;
