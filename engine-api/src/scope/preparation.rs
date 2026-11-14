//! Internal preparation of multiple confirmed current-volume directory roots.
//!
//! This is not the public `scope.review` or `scan.start` contract. Callers own
//! digest calculation and must check the review binding before using handles.
//! The current input subset has no exclusions or explicitly included mounts.
use du_core_contracts::{
    OsPath,
    analysis_ipc::{ScopeBoundary, ScopeDraft},
    scan_observation::{OpenedScopeRoot, OptionalRecentScopeObserver, ScopeRootPort},
};
use du_scan_engine::scope::{Notice, Path};

/// Supplies independently verified volume-relative comparison and boundary facts.
///
/// The implementation must derive facts from the opened root and OS capability
/// evidence. It must never mark alias, comparison or boundary facts confirmed
/// merely because paths or display strings look alike.
pub trait RootEvidence: Send + Sync {
    fn observe(&self, root: &OpenedScopeRoot) -> Result<Path, Error>;
}

/// Uses raw canonical bytes when no volume comparison rules are available.
pub struct RawRootEvidence;

impl RootEvidence for RawRootEvidence {
    fn observe(&self, root: &OpenedScopeRoot) -> Result<Path, Error> {
        let canonical = raw(&root.canonical_path)?;
        Ok(Path {
            volume: Some(root.volume_id.clone()),
            entry: canonical.clone(),
            alias_verified: false,
            boundary_verified: false,
            key: vec![canonical],
            comparison_verified: false,
        })
    }
}

/// Preparation rejected incomplete, changed or unsupported input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Empty,
    UnsupportedScope,
    InvalidPath,
    Unavailable,
    InvalidEvidence,
    ConflictingIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Witness {
    requested: Vec<u8>,
    canonical: Vec<u8>,
    canonical_path: OsPath,
    volume: Vec<u8>,
    file: Vec<u8>,
    facts: Path,
}

struct Opened {
    witness: Witness,
    root: OpenedScopeRoot,
    recent: OptionalRecentScopeObserver,
}

struct Retained {
    facts: Path,
    root: OpenedScopeRoot,
    recent: OptionalRecentScopeObserver,
}

/// Prepared roots and all original evidence from the same observation pass.
pub struct Prepared {
    witnesses: Vec<Witness>,
    retained: Vec<Retained>,
    notices: Vec<Notice>,
    // Absorbed roots continue holding their identity and ancestor handles until
    // review evidence has been checked and the caller consumes this value.
    _absorbed: Vec<OpenedScopeRoot>,
}

/// Read-only evidence for one originally selected root, including absorbed roots.
pub struct SelectedRoot<'a> {
    pub requested_raw: &'a [u8],
    pub canonical_path: &'a OsPath,
    pub volume_id: &'a [u8],
    pub file_id: &'a [u8],
    pub facts: &'a Path,
}

/// A retained root's canonical path and volume-relative comparison components.
pub struct NormalizedRoot<'a> {
    pub canonical_path: &'a OsPath,
    pub comparison_components: &'a [Vec<u8>],
    pub comparison_verified: bool,
    pub facts: &'a Path,
}

impl Prepared {
    /// Uncertainty and identity-backed merge notices.
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    /// Returns all original root bindings in deterministic order for later review hashing.
    pub fn selected_roots(&self) -> impl Iterator<Item = SelectedRoot<'_>> {
        self.witnesses.iter().map(|witness| SelectedRoot {
            requested_raw: &witness.requested,
            canonical_path: &witness.canonical_path,
            volume_id: &witness.volume,
            file_id: &witness.file,
            facts: &witness.facts,
        })
    }

    /// Returns normalized facts and canonical paths in the same order as retained handles.
    ///
    /// Path encoding is available through `canonical_path.encoding`. Components
    /// are volume-relative comparison evidence, not an invented full raw path.
    pub fn normalized_roots(&self) -> impl Iterator<Item = NormalizedRoot<'_>> {
        self.retained.iter().map(|item| NormalizedRoot {
            canonical_path: &item.root.canonical_path,
            comparison_components: &item.facts.key,
            comparison_verified: item.facts.comparison_verified,
            facts: &item.facts,
        })
    }

    /// Transfers retained anchored handles after the caller verifies the review digest.
    pub fn into_roots(self) -> Vec<OpenedScopeRoot> {
        self.retained.into_iter().map(|item| item.root).collect()
    }

    /// Transfers the collection roots and aligned same-open deferred observers.
    pub fn into_roots_and_recent(self) -> (Vec<OpenedScopeRoot>, Vec<OptionalRecentScopeObserver>) {
        self.retained
            .into_iter()
            .map(|item| (item.root, item.recent))
            .unzip()
    }
}

/// Opens all selections once and returns normalized handles and original witnesses.
///
/// Review may inspect the evidence then drop the result. Start repeats this
/// pass, compares the separately computed review digest, and uses these same
/// handles only when that comparison succeeds. Failure returns no handles.
pub fn prepare(
    draft: &ScopeDraft,
    roots: &dyn ScopeRootPort,
    evidence: &dyn RootEvidence,
) -> Result<Prepared, Error> {
    prepare_with_recent(draft, roots, evidence, false)
}

/// Opens each root once and optionally asks the adapter for a same-open deferred observer.
pub fn prepare_with_recent(
    draft: &ScopeDraft,
    roots: &dyn ScopeRootPort,
    evidence: &dyn RootEvidence,
    include_recent: bool,
) -> Result<Prepared, Error> {
    let opened = observe_all(draft, roots, evidence, include_recent)?;
    let (selected_indices, notices) = normalize(&opened)?;
    let witnesses = opened.iter().map(|item| item.witness.clone()).collect();
    let mut selected = Vec::with_capacity(selected_indices.len());
    let mut absorbed = Vec::new();
    for (index, item) in opened.into_iter().enumerate() {
        if selected_indices.contains(&index) {
            selected.push(Retained {
                facts: item.witness.facts,
                root: item.root,
                recent: item.recent,
            });
        } else {
            absorbed.push(item.root);
        }
    }
    Ok(Prepared {
        witnesses,
        retained: selected,
        notices,
        _absorbed: absorbed,
    })
}

fn observe_all(
    draft: &ScopeDraft,
    roots: &dyn ScopeRootPort,
    evidence: &dyn RootEvidence,
    include_recent: bool,
) -> Result<Vec<Opened>, Error> {
    if draft.includes.is_empty() {
        return Err(Error::Empty);
    }
    if draft.boundary != ScopeBoundary::CurrentVolume
        || !draft.exclusions.is_empty()
        || !draft.included_mounts.is_empty()
    {
        return Err(Error::UnsupportedScope);
    }
    let mut output = Vec::with_capacity(draft.includes.len());
    for path in &draft.includes {
        let requested = raw(path)?;
        let (root, recent) = if include_recent {
            roots
                .open_root_with_recent(&requested)
                .map_err(|_| Error::Unavailable)?
        } else {
            (
                roots
                    .open_root(&requested)
                    .map_err(|_| Error::Unavailable)?,
                None,
            )
        };
        let canonical_path =
            super::native_path(&root.canonical_path).map_err(|_| Error::InvalidPath)?;
        let canonical = raw(&canonical_path)?;
        if root.volume_id.is_empty() || root.file_id.is_empty() {
            return Err(Error::InvalidEvidence);
        }
        let facts = evidence.observe(&root)?;
        if facts.volume.as_ref() != Some(&root.volume_id) || facts.entry.is_empty() {
            return Err(Error::InvalidEvidence);
        }
        output.push(Opened {
            witness: Witness {
                requested,
                canonical,
                canonical_path,
                volume: root.volume_id.clone(),
                file: root.file_id.clone(),
                facts,
            },
            root,
            recent,
        });
    }
    output.sort_by(|a, b| {
        a.witness
            .canonical
            .cmp(&b.witness.canonical)
            .then_with(|| a.witness.requested.cmp(&b.witness.requested))
    });
    let mut identities = std::collections::BTreeMap::new();
    let mut same_entry = std::collections::BTreeMap::new();
    let mut aliases = std::collections::BTreeMap::new();
    for item in &output {
        if let Some((volume, file)) = identities.insert(
            &item.witness.facts,
            (&item.witness.volume, &item.witness.file),
        ) && (volume != &item.witness.volume || file != &item.witness.file)
        {
            return Err(Error::ConflictingIdentity);
        }
        let facts = &item.witness.facts;
        if facts.boundary_verified && facts.comparison_verified {
            let key = (&item.witness.volume, &facts.entry, &facts.key);
            if same_entry
                .insert(key, &item.witness.file)
                .is_some_and(|file| file != &item.witness.file)
            {
                return Err(Error::ConflictingIdentity);
            }
            if facts.alias_verified {
                let key = (&item.witness.volume, &facts.key);
                if aliases
                    .insert(key, &item.witness.file)
                    .is_some_and(|file| file != &item.witness.file)
                {
                    return Err(Error::ConflictingIdentity);
                }
            }
        }
    }
    Ok(output)
}

fn raw(path: &OsPath) -> Result<Vec<u8>, Error> {
    super::native_path(path)
        .map_err(|_| Error::InvalidPath)?
        .raw
        .bytes()
        .map_err(|_| Error::InvalidPath)
}

fn normalize(opened: &[Opened]) -> Result<(std::collections::BTreeSet<usize>, Vec<Notice>), Error> {
    let mut selected = std::collections::BTreeSet::new();
    let mut notices = Vec::new();
    let mut identities = std::collections::BTreeMap::new();
    for (index, item) in opened.iter().enumerate() {
        identities
            .entry((item.witness.volume.as_slice(), item.witness.file.as_slice()))
            .or_insert(index);
    }
    for (index, item) in opened.iter().enumerate() {
        let facts = &item.witness.facts;
        if !facts.comparison_verified {
            notices.push(Notice::ComparisonUncertain(facts.clone()));
        }
        if !facts.alias_verified {
            notices.push(Notice::AliasUncertain(facts.clone()));
        }
        if item.root.directory.ancestor_file_ids().is_none() {
            notices.push(Notice::BoundaryUncertain);
        }
        if item.root.directory.ancestor_file_ids().is_some_and(|ids| {
            ids.iter()
                .any(|id| id.is_empty() || id == &item.witness.file)
        }) {
            return Err(Error::InvalidEvidence);
        }
        let identity = (item.witness.volume.as_slice(), item.witness.file.as_slice());
        if let Some(&first) = identities.get(&identity).filter(|&&first| first != index) {
            notices.push(
                if item.witness.canonical == opened[first].witness.canonical {
                    Notice::Duplicate(facts.clone())
                } else {
                    Notice::AliasMerged(facts.clone())
                },
            );
            continue;
        }
        let has_ancestor = item.root.directory.ancestor_file_ids().is_some_and(|ids| {
            ids.iter().any(|ancestor| {
                ancestor.as_slice() != identity.1
                    && identities.contains_key(&(identity.0, ancestor.as_slice()))
            })
        });
        if has_ancestor {
            notices.push(Notice::DescendantAbsorbed(facts.clone()));
        } else {
            selected.insert(index);
        }
    }
    validate_coverage(opened, &selected, &identities)?;
    Ok((selected, notices))
}

/// Reject cycles and require every absorbed representative to name a retained ancestor.
fn validate_coverage(
    opened: &[Opened],
    selected: &std::collections::BTreeSet<usize>,
    identities: &std::collections::BTreeMap<(&[u8], &[u8]), usize>,
) -> Result<(), Error> {
    let mut edges = vec![Vec::new(); opened.len()];
    let mut incoming = vec![0usize; opened.len()];
    for &index in identities.values() {
        let item = &opened[index];
        let mut has_retained_ancestor = false;
        if let Some(ancestors) = item.root.directory.ancestor_file_ids() {
            for ancestor in ancestors {
                if let Some(&target) =
                    identities.get(&(item.witness.volume.as_slice(), ancestor.as_slice()))
                {
                    edges[index].push(target);
                    incoming[target] += 1;
                    has_retained_ancestor |= selected.contains(&target);
                }
            }
        }
        if !selected.contains(&index) && !has_retained_ancestor {
            return Err(Error::ConflictingIdentity);
        }
    }
    let mut queue: std::collections::VecDeque<_> = identities
        .values()
        .copied()
        .filter(|&index| incoming[index] == 0)
        .collect();
    let mut visited = 0usize;
    while let Some(index) = queue.pop_front() {
        visited += 1;
        for &target in &edges[index] {
            incoming[target] -= 1;
            if incoming[target] == 0 {
                queue.push_back(target);
            }
        }
    }
    if visited != identities.len() {
        return Err(Error::ConflictingIdentity);
    }
    Ok(())
}
