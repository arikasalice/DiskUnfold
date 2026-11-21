//! Physical size evidence of one observation: a file or the object of an untraversed link.
use super::{AdmissionError, Identities, exact};
use crate::{ResultVersion, SharingEvidence};
use du_core_contracts::{SizeBasis, SizeEvidence, result::ResultVersionRead};

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
) -> Result<Option<u64>, AdmissionError> {
    if *basis == SizeBasis::Logical {
        let item = version
            .items()
            .get(no as usize)
            .ok_or(AdmissionError::NoAttribution)?;
        return Ok(item.size.as_ref().and_then(|v| exact(v, basis)));
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

/// Attributes one file to its sibling on a physical size basis and returns the bytes it
/// adds to that sibling.
///
/// The file must carry the OS-reported allocation, a file identity and a link count.
/// A file reachable from two siblings is refused for both, so no byte is used twice.
/// Links that lie outside the scope do not refuse the file: the link count is the
/// observed total, and the allocation is counted once inside the scope (F03).
///
/// Extent-sharing evidence ([`SharingEvidence`]) matters only when the basis is
/// `UniquePhysical`. F03 sums allocated sizes exactly whatever the sharing evidence is
/// and makes only the unique-physical value unknown (`physical/finish.rs`). The treemap
/// follows the same rule: an allocated partition needs identity and link evidence, not
/// extent-sharing evidence. A scan never establishes that extents are unshared, so
/// demanding it would leave every real allocated partition unattributed.
pub(super) fn physical_file(
    version: &ResultVersion,
    no: u32,
    sibling: u32,
    basis: &SizeBasis,
    identities: &mut Identities,
) -> Result<u64, AdmissionError> {
    let observation = version
        .physical
        .as_ref()
        .and_then(|v| v.get(no))
        .ok_or(AdmissionError::NoAttribution)?;
    let identity = observation
        .identity()
        .filter(|id| !id.volume.is_empty() && !id.file.is_empty())
        .ok_or(AdmissionError::NoAttribution)?;
    let count = observation
        .link_count()
        .filter(|n| *n > 0)
        .ok_or(AdmissionError::NoAttribution)?;
    if *basis == SizeBasis::UniquePhysical && observation.sharing() != SharingEvidence::None {
        return Err(AdmissionError::NoAttribution);
    }
    let size = observation
        .allocated()
        .filter(|v| v.evidence == SizeEvidence::OsReported)
        .and_then(|v| exact(v, &SizeBasis::Allocated))
        .ok_or(AdmissionError::NoAttribution)?;
    let identity = identity.to_owned();
    if let Some((owner, previous, links, seen)) = identities.owners.get_mut(&identity) {
        if *owner != sibling || *previous != size || *links != count || *seen >= count {
            // Neither sibling can claim the shared file, whichever was read first.
            identities.conflicted.extend([*owner, sibling]);
            return Err(AdmissionError::NoAttribution);
        }
        *seen += 1;
        Ok(0)
    } else {
        identities
            .owners
            .insert(identity, (sibling, size, count, 1));
        Ok(size)
    }
}
