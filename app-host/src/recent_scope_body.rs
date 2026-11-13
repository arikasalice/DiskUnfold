//! Encode and validate the F01 version-one recent-scope storage body.
mod paths;

use std::sync::atomic::{AtomicBool, Ordering};

use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    analysis_ipc::{ScopeBoundary, ScopeDraft, ScopeExclusion, ScopeSource},
};
use du_engine_api::{desktop::RecentScopeAdmission, scope::recent_scope_fingerprint_v1};
use serde::{Deserialize, Serialize};

const BODY_VERSION: u32 = 1;
const FINGERPRINT_FORMAT: u32 = 1;

/// A storage validation failure without path data in the error value.
#[derive(Debug, PartialEq, Eq)]
pub enum BodyError {
    Malformed,
    UnsupportedVersion,
    EvidenceUnavailable,
    FingerprintMismatch,
}

#[derive(Deserialize, Serialize)]
#[serde(remote = "OsPath", deny_unknown_fields)]
struct StoredPath {
    encoding: PathEncoding,
    raw: PathBytes,
    display: String,
    lossy: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ComparisonKey {
    encoding: PathEncoding,
    raw: PathBytes,
}

impl ComparisonKey {
    fn from_path(path: &OsPath) -> Self {
        Self {
            encoding: path.encoding.clone(),
            raw: path.raw.clone(),
        }
    }

    fn hash_input(&self) -> OsPath {
        OsPath {
            encoding: self.encoding.clone(),
            raw: self.raw.clone(),
            display: String::new(),
            lossy: false,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Mount {
    #[serde(with = "StoredPath")]
    path: OsPath,
    key: ComparisonKey,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Exclusion {
    Path {
        #[serde(with = "StoredPath")]
        path: OsPath,
        key: ComparisonKey,
    },
    NamePattern {
        pattern: String,
    },
}

impl Exclusion {
    fn from_pair(applied: &ScopeExclusion, hashed: &ScopeExclusion) -> Result<Self, BodyError> {
        match (applied, hashed) {
            (ScopeExclusion::Path { path }, ScopeExclusion::Path { path: key }) => Ok(Self::Path {
                path: path.clone(),
                key: ComparisonKey::from_path(key),
            }),
            (
                ScopeExclusion::NamePattern { pattern },
                ScopeExclusion::NamePattern { pattern: key },
            ) if pattern == key => Ok(Self::NamePattern {
                pattern: pattern.clone(),
            }),
            _ => Err(BodyError::EvidenceUnavailable),
        }
    }

    fn as_rule(&self, hashed: bool) -> ScopeExclusion {
        match self {
            Self::Path { path, key } => ScopeExclusion::Path {
                path: if hashed {
                    key.hash_input()
                } else {
                    path.clone()
                },
            },
            Self::NamePattern { pattern } => ScopeExclusion::NamePattern {
                pattern: pattern.clone(),
            },
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Conditions {
    includes: Vec<ComparisonKey>,
    exclusions: Vec<Exclusion>,
    boundary: ScopeBoundary,
    included_mounts: Vec<Mount>,
}

/// Stored paths and volume identity for a target, in the admitted scope order.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecentTarget {
    #[serde(with = "StoredPath")]
    pub applied_path: OsPath,
    pub volume_id: PathBytes,
    #[serde(with = "StoredPath")]
    pub entry_path: OsPath,
    #[serde(with = "StoredPath")]
    pub relative_path: OsPath,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BodyV1 {
    body_version: u32,
    normalized_scope: Conditions,
    targets: Vec<RecentTarget>,
}

/// The validated body of one recent-scope row.
pub struct RecentScopeBody(BodyV1);

/// Fields prepared together for one storage transaction.
pub struct EncodedRecentScope {
    pub body: String,
    pub fingerprint: [u8; 32],
    pub include_count: u32,
}

impl RecentScopeBody {
    /// Builds a row from a delivered admission and its original root observers.
    /// Cancellation or missing evidence discards the entire candidate.
    pub fn encode_admission(
        admission: &RecentScopeAdmission,
        cancelled: &AtomicBool,
    ) -> Result<EncodedRecentScope, BodyError> {
        let observed = admission
            .observe(cancelled)
            .ok_or(BodyError::EvidenceUnavailable)?;
        let applied = admission.normalized_scope();
        if observed.len() != applied.includes.len() {
            return Err(BodyError::EvidenceUnavailable);
        }
        let targets = applied
            .includes
            .iter()
            .cloned()
            .zip(observed)
            .map(|(applied_path, root)| RecentTarget {
                applied_path,
                volume_id: PathBytes::from_bytes(&root.volume_id),
                entry_path: root.entry_path,
                relative_path: root.relative_path,
            })
            .collect();
        let body = BodyV1::from_conditions(applied, admission.fingerprint_conditions(), targets)?;
        let fingerprint = body.fingerprint()?;
        let hex: String = fingerprint
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if hex != admission.receipt().scope_fingerprint {
            return Err(BodyError::FingerprintMismatch);
        }
        let include_count = u32::try_from(body.targets.len()).map_err(|_| BodyError::Malformed)?;
        let body = serde_json::to_string(&body).map_err(|_| BodyError::Malformed)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(BodyError::EvidenceUnavailable);
        }
        Ok(EncodedRecentScope {
            body,
            fingerprint,
            include_count,
        })
    }

    /// Validates the stored body and row metadata before any current scope review.
    pub fn decode(
        body: &str,
        fingerprint_format: u32,
        fingerprint: &[u8; 32],
        include_count: u32,
    ) -> Result<Self, BodyError> {
        if fingerprint_format != FINGERPRINT_FORMAT {
            return Err(BodyError::UnsupportedVersion);
        }
        let decoded: BodyV1 = serde_json::from_str(body).map_err(|_| BodyError::Malformed)?;
        if decoded.body_version != BODY_VERSION {
            return Err(BodyError::UnsupportedVersion);
        }
        if u32::try_from(decoded.targets.len()) != Ok(include_count) {
            return Err(BodyError::Malformed);
        }
        if &decoded.fingerprint()? != fingerprint {
            return Err(BodyError::FingerprintMismatch);
        }
        Ok(Self(decoded))
    }

    /// Supplies stored observations for a fresh correspondence check.
    pub fn targets(&self) -> &[RecentTarget] {
        &self.0.targets
    }

    /// Rebuilds the original applied scope for a fresh review by the engine.
    pub fn scope_draft(&self) -> ScopeDraft {
        self.0.draft(false)
    }
}

impl BodyV1 {
    fn from_conditions(
        applied: &ScopeDraft,
        hashed: &ScopeDraft,
        targets: Vec<RecentTarget>,
    ) -> Result<Self, BodyError> {
        if applied.includes.len() != targets.len()
            || applied.includes.len() != hashed.includes.len()
            || applied.exclusions.len() != hashed.exclusions.len()
            || applied.included_mounts.len() != hashed.included_mounts.len()
            || applied.boundary != hashed.boundary
            || applied
                .includes
                .iter()
                .zip(&targets)
                .any(|(path, target)| path != &target.applied_path)
        {
            return Err(BodyError::EvidenceUnavailable);
        }
        let exclusions = applied
            .exclusions
            .iter()
            .zip(&hashed.exclusions)
            .map(|(path, key)| Exclusion::from_pair(path, key))
            .collect::<Result<_, _>>()?;
        let included_mounts = applied
            .included_mounts
            .iter()
            .cloned()
            .zip(&hashed.included_mounts)
            .map(|(path, key)| Mount {
                path,
                key: ComparisonKey::from_path(key),
            })
            .collect();
        Ok(Self {
            body_version: BODY_VERSION,
            normalized_scope: Conditions {
                includes: hashed
                    .includes
                    .iter()
                    .map(ComparisonKey::from_path)
                    .collect(),
                exclusions,
                boundary: applied.boundary,
                included_mounts,
            },
            targets,
        })
    }

    fn fingerprint(&self) -> Result<[u8; 32], BodyError> {
        paths::validate(&self.normalized_scope, &self.targets)?;
        recent_scope_fingerprint_v1(&self.draft(true)).map_err(|_| BodyError::Malformed)
    }

    fn draft(&self, hashed: bool) -> ScopeDraft {
        let scope = &self.normalized_scope;
        ScopeDraft {
            includes: if hashed {
                scope
                    .includes
                    .iter()
                    .map(ComparisonKey::hash_input)
                    .collect()
            } else {
                self.targets
                    .iter()
                    .map(|root| root.applied_path.clone())
                    .collect()
            },
            exclusions: scope
                .exclusions
                .iter()
                .map(|rule| rule.as_rule(hashed))
                .collect(),
            boundary: scope.boundary,
            included_mounts: scope
                .included_mounts
                .iter()
                .map(|mount| {
                    if hashed {
                        mount.key.hash_input()
                    } else {
                        mount.path.clone()
                    }
                })
                .collect(),
            source: ScopeSource::Recent,
        }
    }
}

#[cfg(test)]
mod tests;
