//! Preparing path exclusions from injected, verified comparison capabilities.
mod prefix;
#[cfg(test)]
mod proof_tests;

use super::{
    EngineError, ScopeCorrespondence, ScopeDraft, ScopeReview, TargetAccess, Timestamp, digest,
    preparation,
};
use du_core_contracts::{OsPath, scan_observation::FilterDecision};
use du_core_contracts::{
    analysis_ipc::ScopeExclusion,
    scan_observation::{OpenedScopeRoot, ScopeRootPort},
};
use du_job_runtime::JobClock;
use du_scan_engine::scan::PreMetadataFilter;
use prefix::PrefixFilter;
use std::sync::Arc;

/// Comparison identity and deterministic serialized key supplied by an adapter.
#[derive(Clone, Debug)]
pub struct ComparisonPath {
    pub volume: Vec<u8>,
    pub components: Vec<Vec<u8>>,
    pub serialized_key: OsPath,
}

/// Supplies verified, volume-aware path and child-name comparison keys.
///
/// Implementations must be immutable snapshots, provide the same normalization
/// for path components and child names, and include volume identity in serialized
/// keys. Returning no evidence must never be interpreted as a non-match.
pub trait PathComparison: Send + Sync {
    fn path(&self, path: &OsPath) -> Option<ComparisonPath>;
    fn component(&self, root: &ComparisonPath, parent: &[Vec<u8>], raw: &[u8]) -> Option<Vec<u8>>;
}

/// An original native exclusion and the comparison evidence obtained for it.
///
/// Both values come from one provider call; the key is not an OS path and
/// must not be substituted for the original when restoring a scope.
#[derive(Clone, Debug)]
pub(crate) struct VerifiedExclusion {
    original: OsPath,
    comparison: ComparisonPath,
}

#[cfg(test)]
impl VerifiedExclusion {
    pub(crate) fn original(&self) -> &OsPath {
        &self.original
    }
    pub(crate) fn comparison(&self) -> &ComparisonPath {
        &self.comparison
    }
}

/// Freezes verified exclusions in the same order as the retained roots.
pub(crate) struct FrozenExclusions {
    pub roots: Vec<usize>,
    pub root_keys: Vec<OsPath>,
    /// All rules, including ineffective and repeated rules, in input order.
    pub(crate) rules: Vec<VerifiedExclusion>,
    /// At least one rule is disjoint from every normalized inclusion.
    pub ineffective_rule: bool,
    pub filter: Arc<PreMetadataFilter<'static>>,
}

/// Resolves all paths before producing a filter; missing evidence rejects preparation.
#[cfg(test)]
pub(crate) fn prepare(
    evidence: Arc<dyn PathComparison>,
    roots: &[(OsPath, Vec<u8>)],
    rules: &[OsPath],
) -> Option<FrozenExclusions> {
    let checked = rules
        .iter()
        .map(|original| Some((original.clone(), super::windows_path(original).ok()?)))
        .collect::<Option<Vec<_>>>()?;
    prepare_checked(evidence, roots, &checked)
}

fn prepare_checked(
    evidence: Arc<dyn PathComparison>,
    roots: &[(OsPath, Vec<u8>)],
    rules: &[(OsPath, OsPath)],
) -> Option<FrozenExclusions> {
    let root_keys = roots
        .iter()
        .map(|(path, volume)| {
            let key = evidence.path(path)?;
            (!key.volume.is_empty() && &key.volume == volume).then_some(key)
        })
        .collect::<Option<Vec<_>>>()?;
    let bound_rules = rules
        .iter()
        .map(|(original, checked)| {
            Some(VerifiedExclusion {
                original: original.clone(),
                comparison: evidence.path(checked)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if root_keys.iter().zip(roots).any(|(key, (path, _))| {
        key.serialized_key.encoding != path.encoding
            || key
                .serialized_key
                .raw
                .bytes()
                .map_or(true, |bytes| bytes.is_empty())
    }) || bound_rules.iter().any(|rule| {
        let key = &rule.comparison;
        let path = &rule.original;
        key.volume.is_empty()
            || key.serialized_key.encoding != path.encoding
            || key
                .serialized_key
                .raw
                .bytes()
                .map_or(true, |bytes| bytes.is_empty())
    }) {
        return None;
    }
    let mut roots: Vec<_> = root_keys
        .iter()
        .enumerate()
        .filter(|(_, root)| {
            !bound_rules.iter().any(|rule| {
                let rule = &rule.comparison;
                rule.volume == root.volume && root.components.starts_with(&rule.components)
            })
        })
        .map(|(index, _)| index)
        .collect();
    let ineffective_rule = bound_rules.iter().any(|rule| {
        let rule = &rule.comparison;
        !root_keys.iter().any(|root| {
            root.volume == rule.volume
                && (root.components.starts_with(&rule.components)
                    || rule.components.starts_with(&root.components))
        })
    });
    let sort_keys = root_keys
        .iter()
        .map(|root| root.serialized_key.raw.bytes().ok())
        .collect::<Option<Vec<_>>>()?;
    roots.sort_by(|&a, &b| sort_keys[a].cmp(&sort_keys[b]));
    let kept: Vec<_> = roots
        .iter()
        .map(|&index| root_keys[index].clone())
        .collect();
    let per_root: Vec<_> = kept
        .iter()
        .map(|root| {
            PrefixFilter::new(bound_rules.iter().filter_map(|rule| {
                let rule = &rule.comparison;
                (rule.volume == root.volume)
                    .then(|| rule.components.strip_prefix(root.components.as_slice()))
                    .flatten()
            }))
        })
        .collect();
    let output_roots = kept
        .iter()
        .map(|root| root.serialized_key.clone())
        .collect();
    let filter = Arc::new(move |index: u32, parent: &[Vec<u8>], name: &[u8]| {
        let Some(root) = kept.get(index as usize) else {
            return FilterDecision::Undecidable;
        };
        let Some(rules) = per_root.get(index as usize) else {
            return FilterDecision::Undecidable;
        };
        rules.decide(parent, name, |index, raw| {
            evidence.component(root, &parent[..index], raw)
        })
    });
    Some(FrozenExclusions {
        roots,
        root_keys: output_roots,
        rules: bound_rules,
        ineffective_rule,
        filter,
    })
}

struct PreparedScope {
    roots: Vec<OpenedScopeRoot>,
    targets: Vec<ScopeCorrespondence>,
    notices: Vec<String>,
    review: String,
    filter: Arc<PreMetadataFilter<'static>>,
    proof: BoundScopeProof,
}

/// Immutable rule provenance and the exact comparison conditions that were hashed.
pub(crate) struct BoundScopeProof {
    root_pairs: Vec<(OsPath, OsPath)>,
    rules: Vec<VerifiedExclusion>,
    boundary: du_core_contracts::analysis_ipc::ScopeBoundary,
    included_mounts: Vec<OsPath>,
    source: du_core_contracts::analysis_ipc::ScopeSource,
}

impl BoundScopeProof {
    fn draft(&self, keys: bool) -> ScopeDraft {
        ScopeDraft {
            includes: self
                .root_pairs
                .iter()
                .map(|(original, key)| if keys { key.clone() } else { original.clone() })
                .collect(),
            exclusions: self
                .rules
                .iter()
                .map(|rule| ScopeExclusion::Path {
                    path: if keys {
                        rule.comparison.serialized_key.clone()
                    } else {
                        rule.original.clone()
                    },
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
    evidence: Arc<dyn PathComparison>,
) -> Result<PreparedScope, EngineError> {
    let mut plain = draft.clone();
    let rules = plain
        .exclusions
        .iter()
        .map(|rule| match rule {
            ScopeExclusion::Path { path } => {
                super::windows_path(path).map(|checked| (path.clone(), checked))
            }
            _ => Err(EngineError::reason("scope.exclude_invalid")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    plain.exclusions.clear();
    let prepared = preparation::prepare(&plain, roots, &preparation::RawRootEvidence)
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
    let frozen = prepare_checked(evidence.clone(), &candidates, &rules)
        .ok_or_else(|| EngineError::reason("scope.target_unavailable"))?;
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
            let key = evidence
                .path(root.canonical_path)
                .ok_or_else(|| EngineError::reason("scope.target_unavailable"))?;
            if key.volume != root.volume_id {
                return Err(EngineError::reason("scope.target_unavailable"));
            }
            Ok((
                key.serialized_key,
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
        let raw = path
            .raw
            .bytes()
            .map_err(|_| EngineError::reason("scope.target_unavailable"))?;
        if let Some(previous) = witness_keys.insert(raw, key)
            && (previous.encoding != key.encoding || previous.raw != key.raw)
        {
            return Err(EngineError::reason("scope.target_unavailable"));
        }
    }
    // The reviewed root evidence must use the same comparison key as the
    // frozen filter and scope digest. An inconsistent provider is not proof.
    for (&index, expected) in frozen.roots.iter().zip(&frozen.root_keys) {
        let path = candidates[index]
            .0
            .raw
            .bytes()
            .map_err(|_| EngineError::reason("scope.target_unavailable"))?;
        let actual = witness_keys
            .get(&path)
            .ok_or_else(|| EngineError::reason("scope.target_unavailable"))?;
        if actual.encoding != expected.encoding || actual.raw != expected.raw {
            return Err(EngineError::reason("scope.target_unavailable"));
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
    let mut handles = prepared
        .into_roots()
        .into_iter()
        .map(Some)
        .collect::<Vec<_>>();
    let retained = frozen
        .roots
        .iter()
        .map(|&i| {
            handles[i]
                .take()
                .ok_or_else(|| EngineError::reason("support.internal_error"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PreparedScope {
        roots: retained,
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
    evidence: Arc<dyn PathComparison>,
) -> Result<ScopeReview, EngineError> {
    let prepared = prepare_scope(&draft, roots, evidence)?;
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
    evidence: Arc<dyn PathComparison>,
) -> Result<super::VerifiedScope, EngineError> {
    let prepared =
        prepare_scope(draft, roots, evidence).map_err(|_| EngineError::reason("scope.changed"))?;
    if prepared.review != expected {
        return Err(EngineError::reason("scope.changed"));
    }
    Ok(super::VerifiedScope {
        roots: prepared.roots,
        scope_fingerprint: super::ScopeFingerprint::Bound(prepared.proof),
        filter: Some(prepared.filter),
    })
}
