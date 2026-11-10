//! Read-only duplicate verification with injected identity-bound I/O and streaming hashing.
//!
//! These interfaces are local to duplicate-engine. OS and job runtime composition
//! must supply the approved input list and the identity-bound read port.
use crate::candidates::{
    self, Exclusion, FileIdentity, Placeholder, PrepareError, SampleRange, Size, Target,
};
pub use crate::read_measurement::ReadMeasurement;
use std::collections::BTreeMap;
/// State observed before a file is selected for verification.
///
/// `modified` is an adapter-defined timestamp value that must use the same resolution
/// and representation for the approval and both observations of a read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub identity: FileIdentity,
    pub size: u64,
    pub modified: u64,
    pub change_stamp: Option<u64>,
}
/// An approved target and its pre-observed state. Missing state is never opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationTarget {
    pub target: Target,
    pub snapshot: Option<Snapshot>,
}
/// Purpose-separated hash invocations must not reuse each other's digest domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashPurpose {
    Sample,
    Full,
}

/// Read failure or a failed identity-bound open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    Failed,
    IdentityChanged,
}

/// An opened file bound to its identity; reads must not follow a replacement path.
pub trait OpenFile {
    /// Reads metadata from the same open file used for content.
    fn snapshot(&mut self) -> Result<Snapshot, ReadError>;
    /// Reads at most `buffer.len()` bytes from the specified file offset.
    fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, ReadError>;
}

/// Opens only the selected item and fails if its identity no longer matches.
///
/// The adapter must prevent placeholder downloads and access-time updates, or fail
/// closed when it cannot provide those guarantees.
pub trait ReadPort {
    fn open_bound<'a>(
        &'a mut self,
        item_no: u32,
        expected: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError>;
}

/// Incremental digest state, scoped to one item and hash purpose.
pub trait Digest {
    fn update(&mut self, bytes: &[u8]);
    fn finish(self: Box<Self>) -> Vec<u8>;
}

/// A pinned registered strategy; production composition must provide a trusted implementation.
pub trait HashStrategy {
    fn id(&self) -> &str;
    fn revision(&self) -> u32;
    fn collision_resistant(&self) -> bool;
    /// Initializes a digest for the requested purpose and the confirmed file size.
    fn start(&self, purpose: HashPurpose, approved_size: u64) -> Box<dyn Digest>;
}

/// Strategies fixed at admission for sample filtering and final full-file verification.
pub struct Strategies<'a> {
    pub sample: &'a dyn HashStrategy,
    pub full: &'a dyn HashStrategy,
}

/// Item disposition. Candidates and deferred items are not verified duplicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Candidate,
    SampleCandidate,
    Distinct,
    PlaceholderDeferred,
    Unchecked,
    ReadFailed,
    Changed,
    Verified,
}

/// One independently identified physical file and its selected path references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedFile {
    pub item_nos: Vec<u32>,
    pub identity: FileIdentity,
    /// Whether the approved and observed state included a change stamp.
    pub change_stamp_available: bool,
}

/// A group formed only from full fingerprints produced by the pinned strategy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedGroup {
    pub size: u64,
    pub digest: Vec<u8>,
    pub files: Vec<VerifiedFile>,
}

/// Partial or complete in-memory result; cancellation leaves unfinished candidates unverified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub states: BTreeMap<u32, State>,
    pub groups: Vec<VerifiedGroup>,
    pub cancelled: bool,
    /// Bytes returned by sample reads, including discarded in-flight work.
    pub sample_reads: ReadMeasurement,
    /// Bytes returned by full reads and EOF probes, including discarded work.
    pub full_reads: ReadMeasurement,
    pub algorithm_id: String,
    pub algorithm_revision: u32,
    pub sample_algorithm_id: String,
    pub sample_algorithm_revision: u32,
}

/// Failure before any file is opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    InvalidTargets(PrepareError),
    UnsupportedHash,
}

struct Candidate<'a> {
    approved: &'a VerificationTarget,
    aliases: Vec<u32>,
    identity: FileIdentity,
}

/// Verifies candidate files using only the supplied approved targets and injected ports.
///
/// Missing identity, metadata or placeholder state is deferred without opening. A file
/// already being read reaches its file boundary on cancellation; its digest is discarded.
/// The caller must supply a correctly registered collision-resistant strategy and a port
/// that enforces identity-bound opens. The returned result is not automatically published.
pub fn verify(
    approved: &[VerificationTarget],
    reader: &mut impl ReadPort,
    strategies: Strategies<'_>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Outcome, VerifyError> {
    if strategies.full.id().is_empty()
        || strategies.full.revision() == 0
        || !strategies.full.collision_resistant()
        || strategies.sample.id().is_empty()
        || strategies.sample.revision() == 0
    {
        return Err(VerifyError::UnsupportedHash);
    }
    let inputs: Vec<_> = approved.iter().map(|item| item.target.clone()).collect();
    let prepared = candidates::prepare(&inputs).map_err(VerifyError::InvalidTargets)?;
    let source: BTreeMap<u32, &VerificationTarget> = approved
        .iter()
        .map(|item| (item.target.item_no, item))
        .collect();
    let mut output = Outcome {
        states: approved
            .iter()
            .map(|item| (item.target.item_no, initial_state(item)))
            .collect(),
        groups: Vec::new(),
        cancelled: false,
        sample_reads: ReadMeasurement::default(),
        full_reads: ReadMeasurement::default(),
        algorithm_id: strategies.full.id().to_owned(),
        algorithm_revision: strategies.full.revision(),
        sample_algorithm_id: strategies.sample.id().to_owned(),
        sample_algorithm_revision: strategies.sample.revision(),
    };
    for excluded in prepared.excluded {
        let state = if excluded.reason == Exclusion::PlaceholderDownloadRequired {
            State::PlaceholderDeferred
        } else {
            State::Unchecked
        };
        output.states.insert(excluded.item_no, state);
    }
    if cancelled() {
        output.cancelled = true;
        return Ok(output);
    }
    for group in prepared.groups {
        let group_size = group.files.len();
        let mut selected = Vec::new();
        for file in group.files {
            let Some(&item_no) = file.item_nos.first() else {
                continue;
            };
            let Some(item) = source.get(&item_no).copied() else {
                continue;
            };
            let identity = file.identity.filter(|identity| {
                !file.placeholder_unknown
                    && file.item_nos.iter().all(|number| {
                        source.get(number).is_some_and(|alias| {
                            valid_approval(alias, identity, file.size)
                                && alias.snapshot == item.snapshot
                        })
                    })
            });
            match identity {
                Some(identity) => selected.push(Candidate {
                    approved: item,
                    aliases: file.item_nos,
                    identity,
                }),
                None => set_state(&mut output, &file.item_nos, State::Unchecked),
            }
        }
        if selected.len() >= 2
            && !process_group(
                &selected,
                group.size,
                selected.len() == group_size,
                reader,
                &strategies,
                &mut cancelled,
                &mut output,
            )
        {
            output.cancelled = true;
            break;
        }
    }
    output.groups.sort_by(|left, right| {
        right.size.cmp(&left.size).then_with(|| {
            let first = |group: &VerifiedGroup| {
                group
                    .files
                    .first()
                    .and_then(|file| file.item_nos.first())
                    .copied()
                    .unwrap_or(u32::MAX)
            };
            first(left).cmp(&first(right))
        })
    });
    Ok(output)
}
fn initial_state(item: &VerificationTarget) -> State {
    match (&item.target.identity, item.target.size) {
        (Some(identity), Size::Exact(size)) if valid_approval(item, identity, size) => {
            State::Candidate
        }
        _ => State::Unchecked,
    }
}
fn valid_approval(item: &VerificationTarget, identity: &FileIdentity, size: u64) -> bool {
    item.target.placeholder == Placeholder::Local
        && item.target.size == Size::Exact(size)
        && item.target.identity.as_ref() == Some(identity)
        && item
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.identity == *identity && snapshot.size == size)
}
fn set_state(output: &mut Outcome, items: &[u32], state: State) {
    for item in items {
        output.states.insert(*item, state);
    }
}
fn process_group(
    targets: &[Candidate<'_>],
    size: u64,
    all_eligible: bool,
    reader: &mut impl ReadPort,
    strategies: &Strategies<'_>,
    cancelled: &mut impl FnMut() -> bool,
    output: &mut Outcome,
) -> bool {
    let mut buckets: BTreeMap<Vec<u8>, Vec<&Candidate<'_>>> = BTreeMap::new();
    if candidates::sample_ranges(size).is_some() {
        for target in targets {
            if cancelled() {
                return false;
            }
            let digest = read_digest(
                target,
                HashPurpose::Sample,
                reader,
                strategies.sample,
                &mut output.sample_reads,
            );
            if cancelled() {
                return false;
            }
            match digest {
                Ok(digest) => buckets.entry(digest).or_default().push(target),
                Err(state) => set_state(output, &target.aliases, state),
            }
        }
    } else {
        buckets.insert(Vec::new(), targets.iter().collect());
    }
    let all_compared =
        all_eligible && buckets.values().map(Vec::len).sum::<usize>() == targets.len();
    if all_compared {
        mark_distinct(&buckets, targets.len(), output);
    }
    for matched in buckets.values().filter(|items| items.len() >= 2) {
        if candidates::sample_ranges(size).is_some() {
            for item in matched {
                set_state(output, &item.aliases, State::SampleCandidate);
            }
        }
        if !verify_full(
            matched,
            size,
            all_compared,
            reader,
            strategies.full,
            cancelled,
            output,
        ) {
            return false;
        }
    }
    true
}
fn verify_full(
    targets: &[&Candidate<'_>],
    size: u64,
    all_compared: bool,
    reader: &mut impl ReadPort,
    hasher: &dyn HashStrategy,
    cancelled: &mut impl FnMut() -> bool,
    output: &mut Outcome,
) -> bool {
    let mut digests: BTreeMap<Vec<u8>, Vec<&Candidate<'_>>> = BTreeMap::new();
    for target in targets {
        if cancelled() {
            return false;
        }
        let digest = read_digest(
            target,
            HashPurpose::Full,
            reader,
            hasher,
            &mut output.full_reads,
        );
        if cancelled() {
            return false;
        }
        match digest {
            Ok(digest) => digests.entry(digest).or_default().push(target),
            Err(state) => set_state(output, &target.aliases, state),
        }
    }
    if all_compared {
        mark_distinct(&digests, targets.len(), output);
    }
    for (digest, matched) in digests.into_iter().filter(|(_, items)| items.len() >= 2) {
        let mut files: Vec<_> = matched
            .iter()
            .map(|item| VerifiedFile {
                item_nos: item.aliases.clone(),
                identity: item.identity.clone(),
                change_stamp_available: item
                    .approved
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.change_stamp.is_some()),
            })
            .collect();
        files.sort_by_key(|file| file.item_nos.first().copied().unwrap_or(u32::MAX));
        for file in &files {
            set_state(output, &file.item_nos, State::Verified);
        }
        output.groups.push(VerifiedGroup {
            size,
            digest,
            files,
        });
    }
    true
}

/// A singleton digest proves a difference only when every peer at this stage was read.
fn mark_distinct(
    buckets: &BTreeMap<Vec<u8>, Vec<&Candidate<'_>>>,
    expected: usize,
    output: &mut Outcome,
) {
    if buckets.values().map(Vec::len).sum::<usize>() != expected {
        return;
    }
    for bucket in buckets.values() {
        if let [item] = bucket.as_slice() {
            set_state(output, &item.aliases, State::Distinct);
        }
    }
}
fn read_digest(
    target: &Candidate<'_>,
    purpose: HashPurpose,
    reader: &mut impl ReadPort,
    hasher: &dyn HashStrategy,
    measurement: &mut ReadMeasurement,
) -> Result<Vec<u8>, State> {
    let expected = target.approved.snapshot.as_ref().ok_or(State::Unchecked)?;
    let mut file = reader
        .open_bound(target.approved.target.item_no, &target.identity)
        .map_err(read_error)?;
    if file.snapshot().map_err(read_error)? != *expected {
        return Err(State::Changed);
    }
    let mut digest = hasher.start(purpose, expected.size);
    match purpose {
        HashPurpose::Sample => {
            let ranges = candidates::sample_ranges(expected.size).ok_or(State::ReadFailed)?;
            for range in ranges {
                read_range(&mut *file, range, &mut *digest, measurement)?;
            }
        }
        HashPurpose::Full => {
            read_range(
                &mut *file,
                SampleRange {
                    offset: 0,
                    length: expected.size,
                },
                &mut *digest,
                measurement,
            )?;
            let mut probe = [0_u8; 1];
            if measured_read(&mut *file, expected.size, &mut probe, measurement)? != 0 {
                return Err(State::Changed);
            }
        }
    }
    if file.snapshot().map_err(read_error)? != *expected {
        return Err(State::Changed);
    }
    let digest = digest.finish();
    if digest.is_empty() {
        return Err(State::ReadFailed);
    }
    Ok(digest)
}
fn read_range(
    file: &mut dyn OpenFile,
    range: SampleRange,
    digest: &mut dyn Digest,
    measurement: &mut ReadMeasurement,
) -> Result<(), State> {
    let mut remaining = range.length;
    let mut offset = range.offset;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let limit =
            usize::try_from(remaining.min(buffer.len() as u64)).map_err(|_| State::ReadFailed)?;
        let count = measured_read(file, offset, &mut buffer[..limit], measurement)?;
        if count == 0 {
            return Err(State::ReadFailed);
        }
        digest.update(&buffer[..count]);
        let count = count as u64;
        offset = offset.checked_add(count).ok_or(State::ReadFailed)?;
        remaining -= count;
    }
    Ok(())
}
fn measured_read(
    file: &mut dyn OpenFile,
    offset: u64,
    buffer: &mut [u8],
    measurement: &mut ReadMeasurement,
) -> Result<usize, State> {
    match file.read_at(offset, buffer) {
        Ok(count) => {
            measurement.record(count, buffer.len());
            if count > buffer.len() {
                Err(State::ReadFailed)
            } else {
                Ok(count)
            }
        }
        Err(error) => {
            measurement.exact = false;
            Err(read_error(error))
        }
    }
}
fn read_error(error: ReadError) -> State {
    match error {
        ReadError::Failed => State::ReadFailed,
        ReadError::IdentityChanged => State::Changed,
    }
}
