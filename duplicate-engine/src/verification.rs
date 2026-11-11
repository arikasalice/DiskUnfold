//! Read-only duplicate verification with injected identity-bound I/O and streaming hashing.
//!
//! These interfaces are local to duplicate-engine. OS and job runtime composition
//! must supply the approved input list and the identity-bound read port.
use crate::candidates::{self, Exclusion, PrepareError, Size};
pub use crate::read_measurement::ReadMeasurement;
use std::collections::BTreeMap;

use crate::verification_input::{Candidate, valid_approval};
pub use crate::verification_input::{Snapshot, VerificationTarget};
pub use crate::verification_ports::{
    Digest, HashPurpose, HashStrategy, OpenFile, ReadError, ReadPort, Strategies,
};
use crate::verification_read::read_digest;
pub(crate) use crate::verification_read::verified_full_for_reuse;
pub use crate::verification_result::{
    DistinctStage, FingerprintEvidence, Outcome, State, VerifiedFile, VerifiedGroup,
};

/// Only crate-owned implementations may provide fingerprints to the verifier.
pub(crate) trait FullReuse {
    fn lookup(
        &self,
        target: &VerificationTarget,
        reader: &mut dyn ReadPort,
    ) -> Option<(Vec<u8>, i64)>;
    fn record(&self, target: &VerificationTarget, digest: &[u8]) -> Option<i64>;
}

struct VerificationConfig<'a> {
    strategies: Strategies<'a>,
    reuse: Option<&'a dyn FullReuse>,
}

/// Failure before any file is opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    InvalidTargets(PrepareError),
    UnsupportedHash,
}

/// Verifies candidate files using only the supplied approved targets and injected ports.
///
/// Missing identity, metadata or placeholder state is deferred without opening. A file
/// already being read reaches its file boundary on cancellation; its digest is discarded.
/// The caller must supply a complete comparison population, a correctly registered
/// collision-resistant strategy and a port that enforces identity-bound opens. The returned result is not automatically published.
pub fn verify(
    approved: &[VerificationTarget],
    reader: &mut impl ReadPort,
    strategies: Strategies<'_>,
    cancelled: impl FnMut() -> bool,
) -> Result<Outcome, VerifyError> {
    verify_for_scope(approved, reader, strategies, true, cancelled)
}

pub(crate) fn verify_for_scope(
    approved: &[VerificationTarget],
    reader: &mut impl ReadPort,
    strategies: Strategies<'_>,
    scope_proven: bool,
    cancelled: impl FnMut() -> bool,
) -> Result<Outcome, VerifyError> {
    verify_for_scope_with_reuse(approved, reader, strategies, scope_proven, None, cancelled)
}

pub(crate) fn verify_for_scope_with_reuse(
    approved: &[VerificationTarget],
    reader: &mut impl ReadPort,
    strategies: Strategies<'_>,
    scope_proven: bool,
    reuse: Option<&dyn FullReuse>,
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
        distinct_by: BTreeMap::new(),
        groups: Vec::new(),
        cancelled: false,
        unjudged: Vec::new(),
        interrupted: Vec::new(),
        sample_reads: ReadMeasurement::default(),
        full_reads: ReadMeasurement::default(),
        full_fingerprints: BTreeMap::new(),
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
        output.mark_cancelled();
        return Ok(output);
    }
    let sizes = crate::size_evidence::Evidence::new(&inputs);
    if !crate::size_evidence::settle_unpaired(
        prepared.unpaired,
        &sizes,
        &source,
        scope_proven,
        &mut cancelled,
        &mut output,
    ) {
        output.mark_cancelled();
        return Ok(output);
    }
    let config = VerificationConfig { strategies, reuse };
    for group in prepared.groups {
        let group_size = group.files.len();
        let path_count = group.files.iter().map(|file| file.item_nos.len()).sum();
        let all_peers_known = scope_proven && sizes.no_unrepresented_peer(group.size, path_count);
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
                selected.len() == group_size && all_peers_known,
                reader,
                &config,
                &mut cancelled,
                &mut output,
            )
        {
            output.mark_cancelled();
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
    config: &VerificationConfig<'_>,
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
                config.strategies.sample,
                &mut output.sample_reads,
            );
            if cancelled() {
                output.interrupted.extend_from_slice(&target.aliases);
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
        mark_distinct(&buckets, targets.len(), DistinctStage::Sample, output);
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
            config,
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
    config: &VerificationConfig<'_>,
    cancelled: &mut impl FnMut() -> bool,
    output: &mut Outcome,
) -> bool {
    let mut digests: BTreeMap<Vec<u8>, Vec<&Candidate<'_>>> = BTreeMap::new();
    for target in targets {
        if cancelled() {
            return false;
        }
        let reused = config
            .reuse
            .and_then(|reuse| reuse.lookup(target.approved, reader));
        let digest = match reused.as_ref() {
            Some((digest, _)) => Ok(digest.clone()),
            None => read_digest(
                target,
                HashPurpose::Full,
                reader,
                config.strategies.full,
                &mut output.full_reads,
            ),
        };
        if cancelled() {
            output.interrupted.extend_from_slice(&target.aliases);
            return false;
        }
        match digest {
            Ok(digest) => {
                let (reused, verified_at_ms) = match reused {
                    Some((_, time)) => (true, Some(time)),
                    None => (
                        false,
                        config
                            .reuse
                            .and_then(|cache| cache.record(target.approved, &digest)),
                    ),
                };
                for item in &target.aliases {
                    output.full_fingerprints.insert(
                        *item,
                        FingerprintEvidence {
                            reused,
                            verified_at_ms,
                        },
                    );
                }
                digests.entry(digest).or_default().push(target)
            }
            Err(state) => set_state(output, &target.aliases, state),
        }
    }
    if all_compared {
        mark_distinct(&digests, targets.len(), DistinctStage::Full, output);
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
    stage: DistinctStage,
    output: &mut Outcome,
) {
    if buckets.values().map(Vec::len).sum::<usize>() != expected {
        return;
    }
    for bucket in buckets.values() {
        if let [item] = bucket.as_slice() {
            set_state(output, &item.aliases, State::Distinct);
            for number in &item.aliases {
                output.distinct_by.insert(*number, stage);
            }
        }
    }
}
