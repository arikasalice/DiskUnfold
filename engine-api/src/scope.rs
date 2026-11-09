//! Review a desktop scope using evidence obtained from the opened directory.
use std::sync::Arc;

use du_core_contracts::{
    OsPath, PathEncoding, Timestamp,
    analysis_ipc::{
        ScopeBoundary, ScopeCorrespondence, ScopeDraft, ScopeReview, ScopeReviewRequest,
        ScopeSource, TargetAccess,
    },
    scan_observation::{OpenedScopeRoot, ScopeRootPort},
};
use du_job_runtime::JobClock;
use du_scan_engine::target::{AccessObservation, AccessStatus, access_result};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

/// Registers read-only review for a single local directory on its current volume.
pub fn register_review(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    clock: Arc<dyn JobClock>,
) {
    builder.register(Review { roots, clock });
}

struct Review {
    roots: Arc<dyn ScopeRootPort>,
    clock: Arc<dyn JobClock>,
}

impl UseCase for Review {
    const NAME: &'static str = "scope.review";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ScopeReviewRequest;
    type Response = ScopeReview;

    fn check_scope(&self, request: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        validate(&request.draft).map(|_| ())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let mut draft = request.draft;
        let mut path = validate(&draft)?;
        let raw = path
            .raw
            .bytes()
            .map_err(|_| EngineError::reason("path.unresolvable"))?;
        let opened = self.roots.open_root(&raw);
        let (review_fingerprint, access, reason_key) = match opened {
            Ok(root) => {
                let digest = fingerprint(&root)?;
                path = root.canonical_path;
                (digest, TargetAccess::Accessible, None)
            }
            Err(error) => {
                let digest = hash_fields(&[&raw, &[], &[]]);
                let result = access_result(AccessObservation::Completed(Err(error)));
                (digest, target_access(result.status), result.reason)
            }
        };
        draft.includes = vec![path.clone()];
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

fn validate(draft: &ScopeDraft) -> Result<OsPath, EngineError> {
    if !draft.exclusions.is_empty() {
        return Err(EngineError::reason("scope.exclude_invalid"));
    }
    let Some(path) = draft.includes.first() else {
        return Err(EngineError::reason("scope.empty"));
    };
    if draft.includes.len() != 1
        || draft.source != ScopeSource::Gui
        || draft.boundary != ScopeBoundary::CurrentVolume
        || !draft.included_mounts.is_empty()
    {
        return Err(EngineError::reason("scope.target_unavailable"));
    }
    windows_path(path)
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

/// Revalidates the review against fresh handle evidence before starting work.
pub(crate) fn verify(
    draft: &ScopeDraft,
    expected: &str,
    roots: &dyn ScopeRootPort,
) -> Result<OpenedScopeRoot, EngineError> {
    let path = validate(draft)?;
    let raw = path
        .raw
        .bytes()
        .map_err(|_| EngineError::reason("path.unresolvable"))?;
    let root = roots
        .open_root(&raw)
        .map_err(|_| EngineError::reason("scope.changed"))?;
    if fingerprint(&root)? != expected {
        return Err(EngineError::reason("scope.changed"));
    }
    Ok(root)
}

fn fingerprint(root: &OpenedScopeRoot) -> Result<String, EngineError> {
    let path = windows_path(&root.canonical_path)?;
    let raw = path
        .raw
        .bytes()
        .map_err(|_| EngineError::reason("path.unresolvable"))?;
    if root.volume_id.is_empty() || root.file_id.is_empty() {
        return Err(EngineError::reason("scope.target_unavailable"));
    }
    Ok(hash_fields(&[&raw, &root.volume_id, &root.file_id]))
}

fn hash_fields(fields: &[&[u8]]) -> String {
    // This wire subset has one inclusion and no exclusions or selected mounts.
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"diskunfold.scope.v1\0current_volume\0");
    for bytes in fields {
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    hasher.finalize().to_hex().to_string()
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
