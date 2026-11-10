//! Preparing path exclusions from injected, verified comparison capabilities.
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

/// Freezes verified exclusions in the same order as the retained roots.
pub struct FrozenExclusions {
    pub roots: Vec<usize>,
    pub root_keys: Vec<OsPath>,
    pub rule_keys: Vec<OsPath>,
    pub filter: Arc<PreMetadataFilter<'static>>,
}

/// Resolves all paths before producing a filter; missing evidence rejects preparation.
pub fn prepare(
    evidence: Arc<dyn PathComparison>,
    roots: &[(OsPath, Vec<u8>)],
    rules: &[OsPath],
) -> Option<FrozenExclusions> {
    let root_keys = roots
        .iter()
        .map(|(path, volume)| {
            let key = evidence.path(path)?;
            (!key.volume.is_empty() && &key.volume == volume).then_some(key)
        })
        .collect::<Option<Vec<_>>>()?;
    let rule_keys = rules
        .iter()
        .map(|path| evidence.path(path))
        .collect::<Option<Vec<_>>>()?;
    if root_keys.iter().zip(roots).any(|(key, (path, _))| {
        key.serialized_key.encoding != path.encoding
            || key
                .serialized_key
                .raw
                .bytes()
                .map_or(true, |bytes| bytes.is_empty())
    }) || rule_keys.iter().zip(rules).any(|(key, path)| {
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
            !rule_keys.iter().any(|rule| {
                rule.volume == root.volume && root.components.starts_with(&rule.components)
            })
        })
        .map(|(index, _)| index)
        .collect();
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
            rule_keys
                .iter()
                .filter(|rule| {
                    rule.volume == root.volume && rule.components.starts_with(&root.components)
                })
                .map(|rule| rule.components[root.components.len()..].to_vec())
                .collect::<Vec<_>>()
        })
        .collect();
    let output_roots = kept
        .iter()
        .map(|root| root.serialized_key.clone())
        .collect();
    let output_rules = rule_keys
        .iter()
        .map(|rule| rule.serialized_key.clone())
        .collect();
    let filter = Arc::new(move |index: u32, parent: &[Vec<u8>], name: &[u8]| {
        let Some(root) = kept.get(index as usize) else {
            return FilterDecision::Undecidable;
        };
        let Some(rules) = per_root.get(index as usize) else {
            return FilterDecision::Undecidable;
        };
        if rules.is_empty() {
            return FilterDecision::Admit;
        }
        let mut path = Vec::with_capacity(parent.len() + 1);
        for (index, raw) in parent
            .iter()
            .map(Vec::as_slice)
            .chain(std::iter::once(name))
            .enumerate()
        {
            let Some(key) = evidence.component(root, &parent[..index], raw) else {
                return FilterDecision::Undecidable;
            };
            path.push(key);
        }
        if rules.iter().any(|rule| path.starts_with(rule)) {
            FilterDecision::Exclude
        } else {
            FilterDecision::Admit
        }
    });
    Some(FrozenExclusions {
        roots,
        root_keys: output_roots,
        rule_keys: output_rules,
        filter,
    })
}

struct PreparedScope {
    draft: ScopeDraft,
    roots: Vec<OpenedScopeRoot>,
    targets: Vec<ScopeCorrespondence>,
    notices: Vec<String>,
    fingerprint: String,
    review: String,
    filter: Arc<PreMetadataFilter<'static>>,
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
            ScopeExclusion::Path { path } => super::windows_path(path),
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
    let notices = prepared
        .notices()
        .iter()
        .map(super::notice_key)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let frozen = prepare(evidence.clone(), &candidates, &rules)
        .ok_or_else(|| EngineError::reason("scope.target_unavailable"))?;
    if frozen.roots.is_empty() {
        return Err(EngineError::reason("scope.empty"));
    }
    let mut output = draft.clone();
    output.includes = frozen
        .roots
        .iter()
        .map(|&i| candidates[i].0.clone())
        .collect();
    let mut key_draft = output.clone();
    key_draft.includes = frozen.root_keys;
    key_draft.exclusions = frozen
        .rule_keys
        .into_iter()
        .map(|path| ScopeExclusion::Path { path })
        .collect();
    let scope = digest::scope(&key_draft)?;
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
        draft: output,
        roots: retained,
        targets,
        notices,
        fingerprint: scope.to_hex().to_string(),
        review,
        filter: frozen.filter,
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
        scope: prepared.draft,
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
        scope_fingerprint: prepared.fingerprint,
        filter: Some(prepared.filter),
    })
}
