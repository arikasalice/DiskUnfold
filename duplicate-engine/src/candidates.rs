//! Read-free preparation of a caller-supplied, already scoped target list.
use std::collections::{BTreeMap, BTreeSet};

/// Native file identity established by the operating system, not a path or content hash.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileIdentity {
    pub volume: Vec<u8>,
    pub file: Vec<u8>,
}

/// Whether the source established that the entry is an ordinary file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Other,
    Unknown,
}

/// Logical size evidence; only exact values may enter a size bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Exact(u64),
    LowerBound(u64),
    Unknown,
}

/// Placeholder capability evidence obtained without opening file content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placeholder {
    Local,
    DownloadRequired,
    Unknown,
}

/// A single item already selected from one pinned result version by the caller.
///
/// `item_no` is unique only within that version. This preparation function does not
/// resolve paths or check that the caller's target list matches a requested scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub item_no: u32,
    pub kind: EntryKind,
    pub size: Size,
    pub identity: Option<FileIdentity>,
    pub placeholder: Placeholder,
}

/// Why a target could not safely enter a candidate size bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exclusion {
    NotRegularFile,
    KindUnknown,
    SizeNotExact,
    PlaceholderDownloadRequired,
    IdentityConflict,
}

/// An excluded item and its read-free reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Excluded {
    pub item_no: u32,
    pub reason: Exclusion,
}

/// Paths known to be one physical file, or one item with unknown identity.
///
/// The first item is the deterministic representative; unknown identity does not
/// establish that it is physically distinct from any other member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalFile {
    pub item_nos: Vec<u32>,
    pub identity: Option<FileIdentity>,
    pub size: u64,
    /// At least one selected path lacked a conclusive placeholder observation.
    pub placeholder_unknown: bool,
}

/// A size bucket with two or more potential physical files, not verified duplicates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SizeGroup {
    pub size: u64,
    pub files: Vec<PhysicalFile>,
}

/// Candidate preparation output, not a content-verified duplicate result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub groups: Vec<SizeGroup>,
    pub unpaired: Vec<PhysicalFile>,
    pub excluded: Vec<Excluded>,
}

/// The target list is malformed and must not be used for a verification job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareError {
    DuplicateItem,
    InvalidIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PhysicalKey {
    Known(FileIdentity),
    Unknown(u32),
}

/// Classifies only the supplied target records without opening files or scanning a scope.
///
/// Conflicting sizes for the same observed identity exclude every eligible path for
/// that identity. Equal sizes and samples are candidates, never verification evidence.
pub fn prepare(targets: &[Target]) -> Result<Prepared, PrepareError> {
    validate(targets)?;
    let mut physical: BTreeMap<PhysicalKey, Vec<&Target>> = BTreeMap::new();
    let mut excluded = Vec::new();
    for target in targets {
        if let Some(reason) = exclusion(target) {
            excluded.push(Excluded {
                item_no: target.item_no,
                reason,
            });
            continue;
        }
        let key = target
            .identity
            .as_ref()
            .map_or(PhysicalKey::Unknown(target.item_no), |identity| {
                PhysicalKey::Known(identity.clone())
            });
        physical.entry(key).or_default().push(target);
    }
    let mut sizes: BTreeMap<u64, Vec<PhysicalFile>> = BTreeMap::new();
    for (key, targets) in physical {
        let Some(file) = physical_file(key, &targets, &mut excluded) else {
            continue;
        };
        sizes.entry(file.size).or_default().push(file);
    }
    let mut groups = Vec::new();
    let mut unpaired = Vec::new();
    for (size, mut files) in sizes.into_iter().rev() {
        files.sort_by_key(|file| file.item_nos.first().copied().unwrap_or(u32::MAX));
        if files.len() == 1 {
            unpaired.extend(files);
        } else {
            groups.push(SizeGroup { size, files });
        }
    }
    excluded.sort_by_key(|item| item.item_no);
    Ok(Prepared {
        groups,
        unpaired,
        excluded,
    })
}

fn validate(targets: &[Target]) -> Result<(), PrepareError> {
    let mut seen = BTreeSet::new();
    for target in targets {
        if !seen.insert(target.item_no) {
            return Err(PrepareError::DuplicateItem);
        }
        if target
            .identity
            .as_ref()
            .is_some_and(|id| id.volume.is_empty() || id.file.is_empty())
        {
            return Err(PrepareError::InvalidIdentity);
        }
    }
    Ok(())
}

fn exclusion(target: &Target) -> Option<Exclusion> {
    match (target.kind, target.placeholder, target.size) {
        (EntryKind::Other, _, _) => Some(Exclusion::NotRegularFile),
        (EntryKind::Unknown, _, _) => Some(Exclusion::KindUnknown),
        (_, Placeholder::DownloadRequired, _) => Some(Exclusion::PlaceholderDownloadRequired),
        (_, _, Size::LowerBound(_) | Size::Unknown) => Some(Exclusion::SizeNotExact),
        _ => None,
    }
}

fn physical_file(
    key: PhysicalKey,
    targets: &[&Target],
    excluded: &mut Vec<Excluded>,
) -> Option<PhysicalFile> {
    let first = targets.first()?;
    let Size::Exact(size) = first.size else {
        return None;
    };
    if targets
        .iter()
        .any(|target| target.size != Size::Exact(size))
    {
        excluded.extend(targets.iter().map(|target| Excluded {
            item_no: target.item_no,
            reason: Exclusion::IdentityConflict,
        }));
        return None;
    }
    let mut item_nos: Vec<_> = targets.iter().map(|target| target.item_no).collect();
    item_nos.sort_unstable();
    let identity = match key {
        PhysicalKey::Known(identity) => Some(identity),
        PhysicalKey::Unknown(_) => None,
    };
    Some(PhysicalFile {
        item_nos,
        identity,
        size,
        placeholder_unknown: targets
            .iter()
            .any(|target| target.placeholder == Placeholder::Unknown),
    })
}

/// A byte interval relative to the start of a selected file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRange {
    pub offset: u64,
    pub length: u64,
}

/// Returns the F09 sample intervals for a file strictly larger than 128 KiB.
///
/// `None` means skip the partial-hash stage, not that the file is verified.
/// Zero-byte candidates still require a successful identity-bound open and full verification.
pub fn sample_ranges(size: u64) -> Option<[SampleRange; 2]> {
    const BLOCK: u64 = 64 * 1024;
    if size <= 2 * BLOCK {
        return None;
    }
    Some([
        SampleRange {
            offset: 0,
            length: BLOCK,
        },
        SampleRange {
            offset: size - BLOCK,
            length: BLOCK,
        },
    ])
}
