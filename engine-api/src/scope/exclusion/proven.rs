//! Path exclusions compared through injected, verified comparison evidence.
//!
//! This is the comparison owner for Windows UTF-16LE scopes, whose case and
//! normalization rules come from the provider rather than from raw bytes.
use super::{BoundRule, Frozen, RuleKeys, invalid, prefix::PrefixFilter};
use crate::{error::EngineError, scope::native_path};
use du_core_contracts::{
    OsPath, PathEncoding, analysis_ipc::ScopeExclusion, scan_observation::FilterDecision,
};
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
        .map(|original| Some((original.clone(), crate::scope::windows_path(original).ok()?)))
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

/// Path rules of a Windows UTF-16LE draft bound to one comparison provider.
pub(super) struct ProvenRules {
    evidence: Arc<dyn PathComparison>,
    /// Each original rule paired with its checked native path.
    rules: Vec<(OsPath, OsPath)>,
}

impl ProvenRules {
    /// Accepts only path rules in the Windows encoding, without filesystem access.
    ///
    /// A malformed path keeps `path.unresolvable`; a name pattern or a path in
    /// another encoding is `scope.exclude_invalid`.
    pub(super) fn admit(
        evidence: Arc<dyn PathComparison>,
        rules: &[ScopeExclusion],
    ) -> Result<Self, EngineError> {
        let rules = rules
            .iter()
            .map(|rule| match rule {
                ScopeExclusion::Path { path } => {
                    let checked = native_path(path)?;
                    if checked.encoding != PathEncoding::WindowsUtf16le {
                        return Err(invalid());
                    }
                    Ok((path.clone(), checked))
                }
                ScopeExclusion::NamePattern { .. } => Err(invalid()),
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { evidence, rules })
    }
}

impl RuleKeys for ProvenRules {
    fn freeze(&self, roots: &[(OsPath, Vec<u8>)]) -> Option<Frozen> {
        let frozen = prepare_checked(self.evidence.clone(), roots, &self.rules)?;
        Some(Frozen {
            roots: frozen.roots,
            root_keys: frozen.root_keys,
            rules: frozen
                .rules
                .into_iter()
                .map(|rule| BoundRule {
                    original: ScopeExclusion::Path {
                        path: rule.original,
                    },
                    key: ScopeExclusion::Path {
                        path: rule.comparison.serialized_key,
                    },
                })
                .collect(),
            ineffective_rule: frozen.ineffective_rule,
            filter: frozen.filter,
        })
    }

    fn root_key(&self, canonical: &OsPath, volume: &[u8]) -> Option<OsPath> {
        let key = self.evidence.path(canonical)?;
        (key.volume == volume).then_some(key.serialized_key)
    }
}
