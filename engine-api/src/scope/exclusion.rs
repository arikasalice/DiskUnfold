//! Exclusion rules prepared by the comparison owner of the draft's encoding.
//!
//! Windows UTF-16LE drafts need an injected, verified comparison provider and
//! accept only path rules. Unix drafts compare raw names byte for byte and also
//! accept name patterns. Rules whose encoding differs from the included paths
//! are rejected.
mod prefix;
#[cfg(test)]
mod proof_tests;
mod proven;
mod unix;
#[cfg(test)]
mod unix_tests;

use super::{
    EngineError, ScopeCorrespondence, ScopeDraft, ScopeReview, TargetAccess, Timestamp, digest,
    preparation,
};
use du_core_contracts::{
    OsPath,
    analysis_ipc::ScopeExclusion,
    scan_observation::{OpenedScopeRoot, ScopeRootPort},
};
use du_job_runtime::JobClock;
use du_scan_engine::scan::PreMetadataFilter;
use std::sync::Arc;

pub use proven::{ComparisonPath, PathComparison};
#[cfg(test)]
pub(crate) use proven::{VerifiedExclusion, prepare};

/// One admitted rule as requested and as hashed, taken from one preparation.
///
/// The key is a comparison input, not an OS path, and must not replace the
/// original when a scope is restored.
#[derive(Clone, Debug)]
pub(crate) struct BoundRule {
    original: ScopeExclusion,
    key: ScopeExclusion,
}

/// Rules frozen against the normalized roots by one comparison owner.
struct Frozen {
    /// Retained candidate indices, ordered by their comparison keys.
    roots: Vec<usize>,
    /// Comparison keys of the retained roots, aligned with `roots`.
    root_keys: Vec<OsPath>,
    /// All rules, including ineffective and repeated rules, in input order.
    rules: Vec<BoundRule>,
    /// At least one path rule is disjoint from every normalized inclusion.
    ineffective_rule: bool,
    filter: Arc<PreMetadataFilter<'static>>,
}

/// Comparison keys and a frozen pre-metadata filter for admitted rules.
///
/// Implementations are immutable. `None` means missing evidence and must
/// never be treated as a non-match.
trait RuleKeys {
    /// Freezes the rules against normalized roots given as canonical path and volume.
    fn freeze(&self, roots: &[(OsPath, Vec<u8>)]) -> Option<Frozen>;
    /// Returns the comparison key of an opened root's canonical path on `volume`.
    fn root_key(&self, canonical: &OsPath, volume: &[u8]) -> Option<OsPath>;
}

fn invalid() -> EngineError {
    EngineError::reason("scope.exclude_invalid")
}

/// Rejects rules that the draft's comparison owner cannot apply, before any I/O.
pub(super) fn check(draft: &ScopeDraft, has_comparison: bool) -> Result<(), EngineError> {
    if unix::applies(draft) {
        return unix::UnixRules::admit(&draft.exclusions).map(|_| ());
    }
    if !has_comparison
        || draft
            .exclusions
            .iter()
            .any(|rule| !matches!(rule, ScopeExclusion::Path { .. }))
    {
        return Err(invalid());
    }
    Ok(())
}

/// Selects the comparison owner and validates every rule before roots are opened.
fn admit(
    draft: &ScopeDraft,
    comparison: Option<Arc<dyn PathComparison>>,
) -> Result<Box<dyn RuleKeys>, EngineError> {
    if unix::applies(draft) {
        return Ok(Box::new(unix::UnixRules::admit(&draft.exclusions)?));
    }
    let evidence = comparison.ok_or_else(invalid)?;
    Ok(Box::new(proven::ProvenRules::admit(
        evidence,
        &draft.exclusions,
    )?))
}

struct PreparedScope {
    roots: Vec<OpenedScopeRoot>,
    recent_observers: Vec<du_core_contracts::scan_observation::OptionalRecentScopeObserver>,
    targets: Vec<ScopeCorrespondence>,
    notices: Vec<String>,
    review: String,
    filter: Arc<PreMetadataFilter<'static>>,
    proof: BoundScopeProof,
}

/// Immutable rule provenance and the exact comparison conditions that were hashed.
pub(crate) struct BoundScopeProof {
    root_pairs: Vec<(OsPath, OsPath)>,
    rules: Vec<BoundRule>,
    boundary: du_core_contracts::analysis_ipc::ScopeBoundary,
    included_mounts: Vec<OsPath>,
    source: du_core_contracts::analysis_ipc::ScopeSource,
}

impl BoundScopeProof {
    pub(super) fn draft(&self, keys: bool) -> ScopeDraft {
        ScopeDraft {
            includes: self
                .root_pairs
                .iter()
                .map(|(original, key)| if keys { key.clone() } else { original.clone() })
                .collect(),
            exclusions: self
                .rules
                .iter()
                .map(|rule| {
                    if keys {
                        rule.key.clone()
                    } else {
                        rule.original.clone()
                    }
                })
                .collect(),
            boundary: self.boundary,
            included_mounts: self.included_mounts.clone(),
            source: self.source,
        }
    }

    /// Derives the exact scope digest from the immutable admitted conditions.
    pub(crate) fn fingerprint(&self) -> Result<String, EngineError> {
        Ok(digest::scope(&self.draft(true))?.to_hex().to_string())
    }
}

fn prepare_scope(
    draft: &ScopeDraft,
    roots: &dyn ScopeRootPort,
    keys: &dyn RuleKeys,
    include_recent: bool,
) -> Result<PreparedScope, EngineError> {
    let unavailable = || EngineError::reason("scope.target_unavailable");
    let mut plain = draft.clone();
    plain.exclusions.clear();
    let prepared = preparation::prepare_with_recent(
        &plain,
        roots,
        &preparation::RawRootEvidence,
        include_recent,
    )
    .map_err(super::preparation_error)?;
    let candidates = prepared
        .normalized_roots()
        .map(|root| {
            (
                root.canonical_path.clone(),
                root.facts.volume.clone().unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    let mut notices = prepared
        .notices()
        .iter()
        .map(super::notice_key)
        .collect::<std::collections::BTreeSet<_>>();
    let frozen = keys.freeze(&candidates).ok_or_else(unavailable)?;
    if frozen.roots.is_empty() {
        return Err(EngineError::reason("scope.empty"));
    }
    if frozen.roots.len() < candidates.len() {
        notices.insert("scope.excluded");
    }
    if frozen.ineffective_rule {
        notices.insert("scope.exclusion_ineffective");
    }
    let root_pairs = frozen
        .roots
        .iter()
        .zip(&frozen.root_keys)
        .map(|(&index, key)| (candidates[index].0.clone(), key.clone()))
        .collect();
    let proof = BoundScopeProof {
        root_pairs,
        rules: frozen.rules,
        boundary: draft.boundary,
        included_mounts: draft.included_mounts.clone(),
        source: draft.source,
    };
    let scope = digest::scope(&proof.draft(true))?;
    let witnesses = prepared
        .selected_roots()
        .map(|root| {
            let key = keys
                .root_key(root.canonical_path, root.volume_id)
                .ok_or_else(unavailable)?;
            Ok((
                key,
                root.volume_id.to_vec(),
                root.file_id.to_vec(),
                root.canonical_path.clone(),
            ))
        })
        .collect::<Result<Vec<_>, EngineError>>()?;
    // Index raw canonical paths, rejecting inconsistent duplicate witnesses.
    let mut witness_keys: std::collections::BTreeMap<Vec<u8>, &OsPath> =
        std::collections::BTreeMap::new();
    for (key, _, _, path) in &witnesses {
        let raw = path.raw.bytes().map_err(|_| unavailable())?;
        if let Some(previous) = witness_keys.insert(raw, key)
            && (previous.encoding != key.encoding || previous.raw != key.raw)
        {
            return Err(unavailable());
        }
    }
    // The reviewed root evidence must use the same comparison key as the
    // frozen filter and scope digest. An inconsistent provider is not proof.
    for (&index, expected) in frozen.roots.iter().zip(&frozen.root_keys) {
        let path = candidates[index].0.raw.bytes().map_err(|_| unavailable())?;
        let actual = witness_keys.get(&path).ok_or_else(unavailable)?;
        if actual.encoding != expected.encoding || actual.raw != expected.raw {
            return Err(unavailable());
        }
    }
    let refs = witnesses
        .iter()
        .map(|(path, volume, file, _)| digest::ReviewRoot {
            path,
            identity: Some((volume, file)),
        })
        .collect::<Vec<_>>();
    let review = digest::review(&scope, &refs)?.to_hex().to_string();
    let targets = witnesses
        .into_iter()
        .map(|(_, _, _, path)| ScopeCorrespondence {
            path,
            access: TargetAccess::Accessible,
            reason_key: None,
        })
        .collect();
    let (opened_roots, mut recent_observers) = prepared.into_roots_and_recent();
    let mut handles = opened_roots.into_iter().map(Some).collect::<Vec<_>>();
    let mut aligned_observers = recent_observers.drain(..).map(Some).collect::<Vec<_>>();
    let retained_indices = frozen.roots.to_vec();
    let retained = retained_indices
        .iter()
        .map(|&i| {
            handles[i]
                .take()
                .ok_or_else(|| EngineError::reason("support.internal_error"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let retained_observers = retained_indices
        .iter()
        .map(|&i| {
            aligned_observers[i]
                .take()
                .ok_or_else(|| EngineError::reason("support.internal_error"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PreparedScope {
        roots: retained,
        recent_observers: retained_observers,
        targets,
        notices: notices.into_iter().map(str::to_owned).collect(),
        review,
        filter: frozen.filter,
        proof,
    })
}

pub(super) fn review(
    draft: ScopeDraft,
    seq: u32,
    roots: &dyn ScopeRootPort,
    clock: &dyn JobClock,
    comparison: Option<Arc<dyn PathComparison>>,
) -> Result<ScopeReview, EngineError> {
    let keys = admit(&draft, comparison)?;
    let prepared = prepare_scope(&draft, roots, &*keys, false)?;
    Ok(ScopeReview {
        scope: prepared.proof.draft(false),
        notices: prepared.notices,
        targets: prepared.targets,
        review_fingerprint: prepared.review,
        observed_at: Timestamp::from_unix_millis(clock.now_ms())
            .ok_or_else(|| EngineError::reason("support.internal_error"))?,
        request_seq: seq,
    })
}

pub(super) fn verify(
    draft: &ScopeDraft,
    expected: &str,
    roots: &dyn ScopeRootPort,
    comparison: Option<Arc<dyn PathComparison>>,
    include_recent: bool,
) -> Result<super::VerifiedScope, EngineError> {
    let changed = || EngineError::reason("scope.changed");
    let keys = admit(draft, comparison).map_err(|_| changed())?;
    let prepared = prepare_scope(draft, roots, &*keys, include_recent).map_err(|_| changed())?;
    if prepared.review != expected {
        return Err(changed());
    }
    Ok(super::VerifiedScope {
        roots: prepared.roots,
        recent_observers: prepared.recent_observers,
        normalized_draft: prepared.proof.draft(false),
        scope_fingerprint: super::ScopeFingerprint::Bound(prepared.proof),
        filter: Some(prepared.filter),
    })
}
