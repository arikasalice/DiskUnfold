//! Review a desktop scope using evidence obtained from the opened directory.
mod digest;
pub mod exclusion;
pub mod preparation;

use std::sync::Arc;

use du_core_contracts::{
    OsPath, PathEncoding, Timestamp,
    analysis_ipc::{
        ScopeBoundary, ScopeCorrespondence, ScopeDraft, ScopeReview, ScopeReviewRequest,
        ScopeSource, TargetAccess,
    },
    scan_observation::{OpenedScopeRoot, OptionalRecentScopeObserver, ScopeRootPort},
};
use du_job_runtime::JobClock;
use du_scan_engine::target::{AccessObservation, AccessStatus, access_result};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

/// Registers read-only review for local directories, each on its current volume.
pub fn register_review(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    clock: Arc<dyn JobClock>,
) {
    builder.register(Review {
        roots,
        clock,
        comparison: None,
    });
}

/// Registers scope review with a verified path-comparison capability.
pub fn register_review_with_comparison(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    clock: Arc<dyn JobClock>,
    comparison: Arc<dyn exclusion::PathComparison>,
) {
    builder.register(Review {
        roots,
        clock,
        comparison: Some(comparison),
    });
}

struct Review {
    roots: Arc<dyn ScopeRootPort>,
    clock: Arc<dyn JobClock>,
    comparison: Option<Arc<dyn exclusion::PathComparison>>,
}

impl UseCase for Review {
    const NAME: &'static str = "scope.review";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ScopeReviewRequest;
    type Response = ScopeReview;

    fn check_scope(&self, request: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        validate(&request.draft, self.comparison.is_some())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let mut draft = request.draft;
        validate(&draft, self.comparison.is_some())?;
        if !draft.exclusions.is_empty() {
            return exclusion::review(
                draft,
                request.request_seq,
                &*self.roots,
                &*self.clock,
                self.comparison
                    .clone()
                    .ok_or_else(|| EngineError::reason("scope.exclude_invalid"))?,
            );
        }
        let [single] = draft.includes.as_slice() else {
            return review_multiple(draft, request.request_seq, &*self.roots, &*self.clock);
        };
        let mut path = windows_path(single)?;
        let raw = path
            .raw
            .bytes()
            .map_err(|_| EngineError::reason("path.unresolvable"))?;
        let opened = self.roots.open_root(&raw);
        if let Ok(root) = &opened {
            path = windows_path(&root.canonical_path)?;
        }
        draft.includes = vec![path.clone()];
        let (_, review_fingerprint) = fingerprints(&draft, &path, opened.as_ref().ok())?;
        let result = access_result(AccessObservation::Completed(opened.map(|_| ())));
        let access = target_access(result.status);
        let reason_key = result.reason;
        let observed_at = Timestamp::from_unix_millis(self.clock.now_ms())
            .ok_or_else(|| EngineError::reason("support.internal_error"))?;
        Ok(ScopeReview {
            scope: draft,
            notices: reason_key.iter().cloned().collect(),
            targets: vec![ScopeCorrespondence {
                path,
                access,
                reason_key,
            }],
            review_fingerprint,
            observed_at,
            request_seq: request.request_seq,
        })
    }
}

fn validate(draft: &ScopeDraft, has_comparison: bool) -> Result<(), EngineError> {
    if !draft.exclusions.is_empty()
        && (!has_comparison
            || draft.exclusions.iter().any(|rule| {
                !matches!(
                    rule,
                    du_core_contracts::analysis_ipc::ScopeExclusion::Path { .. }
                )
            }))
    {
        return Err(EngineError::reason("scope.exclude_invalid"));
    }
    if draft.includes.is_empty() {
        return Err(EngineError::reason("scope.empty"));
    }
    if draft.source != ScopeSource::Gui
        || draft.boundary != ScopeBoundary::CurrentVolume
        || !draft.included_mounts.is_empty()
    {
        return Err(EngineError::reason("scope.target_unavailable"));
    }
    Ok(())
}

fn review_multiple(
    draft: ScopeDraft,
    request_seq: u32,
    roots: &dyn ScopeRootPort,
    clock: &dyn JobClock,
) -> Result<ScopeReview, EngineError> {
    let prepared = preparation::prepare(&draft, roots, &preparation::RawRootEvidence)
        .map_err(preparation_error)?;
    let scope = normalized(&draft, &prepared);
    let (_, review_fingerprint) = prepared_fingerprints(&scope, &prepared)?;
    let correspondences = prepared
        .selected_roots()
        .map(|root| ScopeCorrespondence {
            path: root.canonical_path.clone(),
            access: TargetAccess::Accessible,
            reason_key: None,
        })
        .collect();
    let notices = prepared
        .notices()
        .iter()
        .map(notice_key)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let observed_at = Timestamp::from_unix_millis(clock.now_ms())
        .ok_or_else(|| EngineError::reason("support.internal_error"))?;
    Ok(ScopeReview {
        scope,
        notices,
        targets: correspondences,
        review_fingerprint,
        observed_at,
        request_seq,
    })
}

fn normalized(draft: &ScopeDraft, prepared: &preparation::Prepared) -> ScopeDraft {
    ScopeDraft {
        includes: prepared
            .normalized_roots()
            .map(|root| root.canonical_path.clone())
            .collect(),
        ..draft.clone()
    }
}

fn prepared_fingerprints(
    normalized: &ScopeDraft,
    prepared: &preparation::Prepared,
) -> Result<(blake3::Hash, String), EngineError> {
    let scope = digest::scope(normalized)?;
    let selected: Vec<_> = prepared
        .selected_roots()
        .map(|root| digest::ReviewRoot {
            path: root.canonical_path,
            identity: Some((root.volume_id, root.file_id)),
        })
        .collect();
    let review = digest::review(&scope, &selected)?;
    Ok((scope, review.to_hex().to_string()))
}

fn preparation_error(error: preparation::Error) -> EngineError {
    use preparation::Error;
    EngineError::reason(match error {
        Error::Empty => "scope.empty",
        Error::InvalidPath => "path.unresolvable",
        _ => "scope.target_unavailable",
    })
}

fn notice_key(notice: &du_scan_engine::scope::Notice) -> &'static str {
    use du_scan_engine::scope::Notice;
    match notice {
        Notice::Duplicate(_) => "scope.overlap_merged",
        Notice::AliasMerged(_) => "scope.alias_merged",
        Notice::DescendantAbsorbed(_) => "scope.descendant_absorbed",
        _ => "scope.correspondence_unknown",
    }
}

pub(crate) fn windows_path(path: &OsPath) -> Result<OsPath, EngineError> {
    let invalid = || EngineError::reason("path.unresolvable");
    let raw = path.raw.bytes().map_err(|_| invalid())?;
    let (units, tail) = raw.as_chunks::<2>();
    if path.encoding != PathEncoding::WindowsUtf16le || units.is_empty() || !tail.is_empty() {
        return Err(invalid());
    }
    let wide: Vec<_> = units.iter().copied().map(u16::from_le_bytes).collect();
    if wide.contains(&0) {
        return Err(invalid());
    }
    let display = String::from_utf16(&wide);
    Ok(OsPath {
        encoding: path.encoding.clone(),
        raw: path.raw.clone(),
        lossy: display.is_err(),
        display: display.unwrap_or_else(|_| String::from_utf16_lossy(&wide)),
    })
}

/// An anchored scope and its identity-independent fingerprint.
pub(crate) struct VerifiedScope {
    pub roots: Vec<OpenedScopeRoot>,
    pub recent_observers: Vec<OptionalRecentScopeObserver>,
    pub normalized_draft: ScopeDraft,
    pub scope_fingerprint: ScopeFingerprint,
    pub filter: Option<Arc<du_scan_engine::scan::PreMetadataFilter<'static>>>,
}

/// The verified source of a scope fingerprint for the scan submission.
pub(crate) enum ScopeFingerprint {
    Ready(String),
    Bound(exclusion::BoundScopeProof),
}

impl ScopeFingerprint {
    /// Returns the hash inputs when the scope used verified comparison keys.
    pub(crate) fn retained_comparison_input(&self) -> Option<ScopeDraft> {
        match self {
            Self::Ready(_) => None,
            Self::Bound(proof) => Some(proof.draft(true)),
        }
    }

    pub(crate) fn resolve(self) -> Result<String, EngineError> {
        match self {
            Self::Ready(fingerprint) => Ok(fingerprint),
            Self::Bound(proof) => proof.fingerprint(),
        }
    }
}

/// Revalidates the review against fresh handle evidence before starting work.
pub(crate) fn verify(
    draft: &ScopeDraft,
    expected: &str,
    roots: &dyn ScopeRootPort,
    comparison: Option<Arc<dyn exclusion::PathComparison>>,
    include_recent: bool,
) -> Result<VerifiedScope, EngineError> {
    validate(draft, comparison.is_some())?;
    if !draft.exclusions.is_empty() {
        return exclusion::verify(
            draft,
            expected,
            roots,
            comparison.ok_or_else(|| EngineError::reason("scope.exclude_invalid"))?,
            include_recent,
        );
    }
    let [single] = draft.includes.as_slice() else {
        let prepared = preparation::prepare_with_recent(
            draft,
            roots,
            &preparation::RawRootEvidence,
            include_recent,
        )
        .map_err(|error| match error {
            preparation::Error::InvalidPath => preparation_error(error),
            _ => EngineError::reason("scope.changed"),
        })?;
        let (scope, review) = prepared_fingerprints(&normalized(draft, &prepared), &prepared)?;
        if review != expected {
            return Err(EngineError::reason("scope.changed"));
        }
        let normalized_draft = normalized(draft, &prepared);
        let (roots, recent_observers) = prepared.into_roots_and_recent();
        return Ok(VerifiedScope {
            roots,
            recent_observers,
            normalized_draft,
            scope_fingerprint: ScopeFingerprint::Ready(scope.to_hex().to_string()),
            filter: None,
        });
    };
    let path = windows_path(single)?;
    let raw = path
        .raw
        .bytes()
        .map_err(|_| EngineError::reason("path.unresolvable"))?;
    let (root, recent) = if include_recent {
        roots.open_root_with_recent(&raw)
    } else {
        roots.open_root(&raw).map(|root| (root, None))
    }
    .map_err(|_| EngineError::reason("scope.changed"))?;
    let mut normalized = draft.clone();
    let canonical = windows_path(&root.canonical_path)?;
    normalized.includes = vec![canonical.clone()];
    let (scope_fingerprint, review) = fingerprints(&normalized, &canonical, Some(&root))?;
    if review != expected {
        return Err(EngineError::reason("scope.changed"));
    }
    Ok(VerifiedScope {
        roots: vec![root],
        recent_observers: vec![recent],
        normalized_draft: normalized,
        scope_fingerprint: ScopeFingerprint::Ready(scope_fingerprint.to_hex().to_string()),
        filter: None,
    })
}

fn fingerprints(
    normalized: &ScopeDraft,
    path: &OsPath,
    observed: Option<&OpenedScopeRoot>,
) -> Result<(blake3::Hash, String), EngineError> {
    if observed.is_some_and(|root| root.volume_id.is_empty() || root.file_id.is_empty()) {
        return Err(EngineError::reason("scope.target_unavailable"));
    }
    let scope = digest::scope(normalized)?;
    let review = digest::review(
        &scope,
        &[digest::ReviewRoot {
            path,
            identity: observed.map(|root| (root.volume_id.as_slice(), root.file_id.as_slice())),
        }],
    )?;
    Ok((scope, review.to_hex().to_string()))
}

pub(crate) fn target_access(status: AccessStatus) -> TargetAccess {
    match status {
        AccessStatus::Accessible => TargetAccess::Accessible,
        AccessStatus::Denied => TargetAccess::Denied,
        AccessStatus::Disconnected => TargetAccess::Disconnected,
        AccessStatus::Missing => TargetAccess::Missing,
        AccessStatus::Unresponsive => TargetAccess::Unresponsive,
        AccessStatus::Unknown => TargetAccess::Unknown,
    }
}
