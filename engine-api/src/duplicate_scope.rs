//! Read-only F09 selection from one pinned scan result.
//!
//! Selection accepts item references resolved by the caller against the pinned
//! scan. It does not open file content or authorize subsequent content reads.
use du_analysis_index::{FileIdentity, IngestSink, QueryError, ResultVersion};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, Timestamp,
    result::{
        ResultCompleteness, ResultItemKind, ResultNativePath, ResultScopeRoot, ResultVersionRead,
    },
};
use std::sync::Arc;

/// A duplicate scope resolved to observed item references by a trusted caller.
pub struct Scope<'a> {
    /// The current scan result the caller used to resolve every reference.
    pub version: &'a Id,
    /// Observed directories whose descendants are eligible for selection.
    pub roots: &'a [ItemRef],
    /// Observed files or directories to omit, including their descendants.
    pub exclusions: &'a [ItemRef],
    /// Inclusive minimum logical byte size for an observed file.
    pub min_bytes: u64,
}

/// Original observations, not proof of current identity or read permission.
#[derive(Debug, PartialEq, Eq)]
pub struct Selected {
    /// Item reference in the pinned scan result.
    pub item: ItemRef,
    /// Native path retained by the scan, not a current identity check.
    pub path: ResultNativePath,
    /// Exact observed logical byte size.
    pub size: u64,
    /// File identity observed in the scan.
    pub identity: FileIdentity,
    /// Modification time observed in the scan.
    pub modified: Timestamp,
}

/// Counts cover observed file entries only; they do not count unread ranges.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Observed files outside the selected roots.
    pub outside: u64,
    /// Observed files covered by a request exclusion within the selected roots.
    /// This does not describe exclusions from the original scan.
    pub excluded: u64,
    /// Observed files below the minimum size.
    pub below_minimum: u64,
    /// Observed files missing required metadata.
    pub unavailable: u64,
}

/// The request and source observations captured with one selected scan result.
#[derive(Debug, PartialEq, Eq)]
pub struct SelectionScope {
    /// Root references validated against the pinned result.
    pub roots: Vec<ItemRef>,
    /// Exclusion references validated against the pinned result.
    pub exclusions: Vec<ItemRef>,
    /// Inclusive lower bound on logical bytes.
    pub minimum_bytes: u64,
    /// Observed file counts for the admitted selection.
    pub counts: Counts,
    /// Completeness of the pinned scan result.
    pub source_completeness: ResultCompleteness,
    /// Source observation time when available.
    pub observed_at: Option<Timestamp>,
}

/// Retains the exact source while selected references are in use.
pub struct Selection {
    version: Arc<ResultVersion>,
    files: Vec<Selected>,
    evidence: SelectionScope,
}

impl Selection {
    /// Returns the retained scan result version identifier.
    pub fn version(&self) -> &Id {
        self.version.version_id()
    }
    /// Returns the source result's completeness.
    pub fn completeness(&self) -> ResultCompleteness {
        self.version.completeness()
    }
    /// Only observed items in this selection can be retrieved.
    pub fn selected(&self, item: &ItemRef) -> Option<&Selected> {
        if &item.result_version != self.version.version_id() {
            return None;
        }
        self.files
            .binary_search_by_key(&item.item_no, |selected| selected.item.item_no)
            .ok()
            .and_then(|index| self.files.get(index))
    }
    /// Returns the selected observations in item-number order.
    pub fn files(&self) -> &[Selected] {
        &self.files
    }
    /// Returns the scan-owned root and raw components for a member of this selection.
    /// An adapter must still revalidate the root and the target's identity before reading.
    pub fn anchored_location(
        &self,
        item: &ItemRef,
    ) -> Result<(&ResultScopeRoot, Vec<&[u8]>), Error> {
        if self.selected(item).is_none() {
            return Err(Error::InvalidScope);
        }
        let (root, parts) = self
            .version
            .observed_path_parts(item)
            .map_err(|_| Error::Unavailable)?;
        Ok((root, parts.collect()))
    }
    /// Returns observed-file counts, not counts of omitted scan entries.
    pub fn counts(&self) -> &Counts {
        &self.evidence.counts
    }
    /// Returns the request and source evidence captured with this pinned selection.
    ///
    /// This is historical scope evidence, not authorization to open a file.
    pub fn scope_evidence(&self) -> &SelectionScope {
        &self.evidence
    }
}

#[derive(Debug, PartialEq, Eq)]
/// Selection failed without authorizing any content reads.
pub enum Error {
    /// No current scan result exists.
    MissingVersion,
    /// The requested scan result is no longer current.
    StaleVersion,
    /// Scope inputs are invalid.
    InvalidScope,
    /// Required scan evidence or the source is unavailable.
    Unavailable,
    /// Cancellation discarded the in-progress selection.
    Cancelled,
}

/// Selects files beneath resolved directories in one current scan result.
///
/// Selection follows parent links, so similar display names and raw path
/// prefixes cannot expand the scope. Overlapping roots select each item once.
/// This entry requires a complete source with verified root observations and
/// returns historical evidence for later live identity checks.
///
/// # Errors
/// Returns an error for cancellation, a missing or stale result, invalid or
/// foreign references, or unavailable source evidence.
pub fn select_scope(
    source: &IngestSink,
    scope: Scope<'_>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Selection, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let version = source.pin_current(scope.version).map_err(source_error)?;
    verify_source(&version)?;
    for (index, root) in version.scope_roots().iter().enumerate() {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        verify_anchor(
            &version,
            &ItemRef {
                result_version: version.version_id().clone(),
                item_no: root.item_no,
            },
            index,
        )?;
    }
    select_pinned(version, scope, &mut cancelled)
}

/// Selects observations beneath one scan-owned root, with no new exclusions.
///
/// The caller must supply the application's trusted scan result store. The
/// returned data are historical observations, never permission to read content.
/// This narrow entry point does not accept arbitrary paths or scope item refs.
/// A caller translating a broader request must reject unsupported roots or
/// exclusions rather than dropping them before calling this function.
pub fn select_whole_scan(
    source: &IngestSink,
    expected: &Id,
    min_bytes: u64,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Selection, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let version = source.pin_current(expected).map_err(source_error)?;
    verify_source(&version)?;
    let [scope_root] = version.scope_roots() else {
        return Err(Error::Unavailable);
    };
    let root = ItemRef {
        result_version: version.version_id().clone(),
        item_no: scope_root.item_no,
    };
    verify_anchor(&version, &root, 0)?;
    let roots = [root];
    select_pinned(
        version,
        Scope {
            version: expected,
            roots: &roots,
            exclusions: &[],
            min_bytes,
        },
        &mut cancelled,
    )
}

fn verify_source(version: &ResultVersion) -> Result<(), Error> {
    if version.completeness() != ResultCompleteness::Complete
        || !version.scope_gaps().is_empty()
        || version.scope_roots().len() != version.observed_roots().len()
    {
        return Err(Error::Unavailable);
    }
    Ok(())
}

fn verify_anchor(version: &ResultVersion, root: &ItemRef, index: usize) -> Result<(), Error> {
    let (observed_index, observed) = version
        .observed_root_for(root)
        .map_err(|_| Error::Unavailable)?;
    if observed_index as usize != index
        || observed.volume_id.is_empty()
        || observed.file_id.is_empty()
    {
        return Err(Error::Unavailable);
    }
    version.native_path(root).map_err(|_| Error::Unavailable)?;
    Ok(())
}

#[cfg(test)]
fn select(
    source: &IngestSink,
    scope: Scope<'_>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Selection, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let version = source.pin_current(scope.version).map_err(source_error)?;
    select_pinned(version, scope, &mut cancelled)
}

fn select_pinned(
    version: Arc<ResultVersion>,
    scope: Scope<'_>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Selection, Error> {
    if scope.roots.is_empty() {
        return Err(Error::InvalidScope);
    }
    if version.version_id() != scope.version {
        return Err(Error::StaleVersion);
    }
    let roots = resolve_scope(&version, scope.roots, true, &mut cancelled)?;
    let exclusions = resolve_scope(&version, scope.exclusions, false, &mut cancelled)?;
    let mut membership = vec![0_u8; version.items().len()];
    for root in roots {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        *membership
            .get_mut(root as usize)
            .ok_or(Error::Unavailable)? |= 1;
    }
    for excluded in exclusions {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        *membership
            .get_mut(excluded as usize)
            .ok_or(Error::Unavailable)? |= 2;
    }
    let mut files = Vec::new();
    let mut counts = Counts::default();
    // The index builder numbers parents before children; validate that invariant
    // before inheriting root and exclusion membership from the parent.
    for (index, item) in version.items().iter().enumerate() {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if item.item_no as usize != index {
            return Err(Error::Unavailable);
        }
        if let Some(parent) = item.parent {
            let parent = parent as usize;
            if parent >= index {
                return Err(Error::Unavailable);
            }
            membership[index] |= membership[parent];
        }
        if item.kind != ResultItemKind::File {
            continue;
        }
        if membership[index] & 1 == 0 {
            counts.outside += 1;
            continue;
        }
        if membership[index] & 2 != 0 {
            counts.excluded += 1;
            continue;
        }
        let reference = ItemRef {
            result_version: version.version_id().clone(),
            item_no: item.item_no,
        };
        let Some(size) = exact_size(item.size.as_ref()) else {
            counts.unavailable += 1;
            continue;
        };
        if size < scope.min_bytes {
            counts.below_minimum += 1;
            continue;
        }
        let Some(selected) = observe(&version, reference, size) else {
            counts.unavailable += 1;
            continue;
        };
        if cancelled() {
            return Err(Error::Cancelled);
        }
        files.push(selected);
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let evidence = SelectionScope {
        roots: scope.roots.to_vec(),
        exclusions: scope.exclusions.to_vec(),
        minimum_bytes: scope.min_bytes,
        counts,
        source_completeness: version.completeness(),
        observed_at: version.observed_at().cloned(),
    };
    Ok(Selection {
        version,
        files,
        evidence,
    })
}

fn resolve_scope(
    version: &ResultVersion,
    refs: &[ItemRef],
    roots: bool,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<u32>, Error> {
    refs.iter()
        .map(|reference| {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let detail = version.detail(reference).map_err(|_| Error::InvalidScope)?;
            let allowed = if roots {
                detail.item.kind == ResultItemKind::Directory
            } else {
                matches!(
                    detail.item.kind,
                    ResultItemKind::Directory | ResultItemKind::File
                )
            };
            if allowed {
                Ok(detail.item.item_no)
            } else {
                Err(Error::InvalidScope)
            }
        })
        .collect()
}

fn exact_size(value: Option<&du_core_contracts::SizeValue>) -> Option<u64> {
    let value = value?;
    if value.basis != SizeBasis::Logical
        || !matches!(
            value.evidence,
            SizeEvidence::Measured | SizeEvidence::OsReported
        )
    {
        return None;
    }
    match &value.amount {
        SizeAmount::Exact { bytes } => bytes.as_str().parse().ok(),
        _ => None,
    }
}

fn observe(version: &ResultVersion, item: ItemRef, size: u64) -> Option<Selected> {
    let detail = version.detail(&item).ok()?;
    let identity = detail.physical?.identity.as_ref()?;
    let (_, root) = version.observed_root_for(&item).ok()?;
    if identity.volume.is_empty()
        || identity.file.is_empty()
        || root.file_id.is_empty()
        || root.volume_id != identity.volume
    {
        return None;
    }
    Some(Selected {
        path: version.native_path(&item).ok()?,
        modified: version.item_modified_at(item.item_no)?.clone(),
        identity: identity.clone(),
        item,
        size,
    })
}

fn source_error(error: QueryError) -> Error {
    match error {
        QueryError::NotFound => Error::MissingVersion,
        QueryError::StaleVersion => Error::StaleVersion,
        _ => Error::Unavailable,
    }
}

#[cfg(test)]
mod tests;
