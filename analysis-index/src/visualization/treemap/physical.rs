//! Physical size evidence of one observation: a file or the object of an untraversed link.
use super::{AdmissionError, Identities, exact};
use crate::{ResultVersion, SharingEvidence, physical::pinned_allocation};
use du_core_contracts::{SizeBasis, SizeEvidence, result::ResultVersionRead};

/// What one file adds to its sibling on a physical size basis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FileBytes {
    pub(super) bytes: u64,
    /// F03 makes every total above this file a lower bound: it has no usable allocation
    /// or identity and adds nothing, or its allocation is itself a lower bound.
    pub(super) unmeasured: bool,
}

/// The own size of an untraversed link on a size basis, or `None` when it is unknown.
///
/// On the logical basis it is the link's observed logical size. On a physical basis the
/// link needs the same allocation evidence as a file, and a link that shares it with
/// another sibling is refused like a shared file; without allocation evidence its size is
/// unknown.
pub(super) fn link_bytes(
    version: &ResultVersion,
    no: u32,
    sibling: u32,
    basis: &SizeBasis,
    identities: &mut Identities,
) -> Result<Option<FileBytes>, AdmissionError> {
    if *basis == SizeBasis::Logical {
        let item = version
            .items()
            .get(no as usize)
            .ok_or(AdmissionError::NoAttribution)?;
        return Ok(item
            .size
            .as_ref()
            .and_then(|v| exact(v, basis))
            .map(|bytes| FileBytes {
                bytes,
                unmeasured: false,
            }));
    }
    let allocated = version
        .physical
        .as_ref()
        .and_then(|v| v.get(no))
        .and_then(|observation| observation.allocated())
        .is_some_and(|v| v.evidence == SizeEvidence::OsReported);
    if !allocated {
        return Ok(None);
    }
    physical_file(version, no, sibling, basis, identities).map(Some)
}

/// Attributes one file to its sibling on a physical size basis and returns what it adds.
///
/// The file is read as F03 reads it for its totals (`pinned_allocation`), so the
/// bytes of a sibling are the bytes F03 adds under it (#364). A file F03 adds nothing for
/// leaves the sibling a lower bound rather than refusing it; an unknown link count does
/// not refuse it either. A file reachable from two siblings is refused for both, so no
/// byte is used twice. Links that lie outside the scope do not refuse the file: the link
/// count is the observed total, and the allocation is counted once inside the scope (F03).
///
/// Extent-sharing evidence ([`SharingEvidence`]) matters only when the basis is
/// `UniquePhysical`. F03 sums allocated sizes exactly whatever the sharing evidence is
/// and makes only the unique-physical value unknown (`physical/finish.rs`). The treemap
/// follows the same rule: an allocated partition needs identity evidence, not
/// extent-sharing evidence. A scan never establishes that extents are unshared, so
/// demanding it would leave every real allocated partition unattributed.
pub(super) fn physical_file(
    version: &ResultVersion,
    no: u32,
    sibling: u32,
    basis: &SizeBasis,
    identities: &mut Identities,
) -> Result<FileBytes, AdmissionError> {
    let Some(file) = pinned_allocation(version, no as usize) else {
        return Ok(FileBytes {
            bytes: 0,
            unmeasured: true,
        });
    };
    if *basis == SizeBasis::UniquePhysical && file.sharing != SharingEvidence::None {
        return Err(AdmissionError::NoAttribution);
    }
    let counted = if *basis == SizeBasis::Allocated {
        version
            .shared_counted_item(&super::super::reference(version, no))
            .map_err(|_| AdmissionError::NoAttribution)?
    } else {
        None
    };
    let count = file.link_count;
    let identity = file.identity.to_owned();
    if let Some((owner, previous, links, seen)) = identities.owners.get_mut(&identity) {
        if (*owner != sibling && counted.is_none())
            || *previous != file.bytes
            || *links != count
            || count.is_some_and(|links| *seen >= links)
        {
            // Neither sibling can claim the shared file, whichever was read first.
            identities.conflicted.extend([*owner, sibling]);
            return Err(AdmissionError::NoAttribution);
        }
        *seen += 1;
        Ok(FileBytes {
            bytes: if counted.as_ref().is_none_or(|item| item.item_no == no) {
                file.bytes
            } else {
                0
            },
            unmeasured: file.lower,
        })
    } else {
        identities
            .owners
            .insert(identity, (sibling, file.bytes, count, 1));
        Ok(FileBytes {
            bytes: if counted.as_ref().is_none_or(|item| item.item_no == no) {
                file.bytes
            } else {
                0
            },
            unmeasured: file.lower,
        })
    }
}
