//! Immutable registered verification outcomes, scoped to live source versions.
//!
//! This store does not construct the complete IF-15 response: scope summaries,
//! observation times, result completeness and group presentation remain with the
//! integration that owns those inputs. No retention count is selected here.

use crate::RegisteredOutcome;
use du_core_contracts::{Id, ItemRef, Timestamp, result::ResultCompleteness};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

/// Scope and source observation supplied by the pinned-scan selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeEvidence {
    /// Selected root references from the source result.
    pub roots: Vec<ItemRef>,
    /// Exclusion references from the same source result.
    pub exclusions: Vec<ItemRef>,
    /// Inclusive minimum logical size in bytes.
    pub minimum_bytes: u64,
    /// Observed file counts outside the selected roots.
    pub outside: u64,
    /// Observed files excluded by path.
    pub excluded: u64,
    /// Observed files below the minimum size.
    pub below_minimum: u64,
    /// Observed files lacking selection metadata.
    pub unavailable: u64,
    /// Completeness of the underlying scan result.
    pub source_completeness: ResultCompleteness,
    /// Source observation time when supplied by the scan.
    pub observed_at: Option<Timestamp>,
}

/// Whether all selected content dispositions have conclusive evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentCoverage {
    /// Every selected item has a verified or stage-distinct disposition.
    Complete,
    /// Some content or scope evidence remains unavailable or unjudged.
    Partial,
    /// The terminal gate selected a cancelled result.
    Cancelled,
}

/// Result data prepared before entering the runtime terminal gate.
pub(crate) struct PreparedResult {
    outcome: RegisteredOutcome,
    scope: ScopeEvidence,
    coverage: ContentCoverage,
    unjudged: Vec<u32>,
}

impl PreparedResult {
    pub(crate) fn new(mut outcome: RegisteredOutcome, scope: ScopeEvidence) -> Self {
        let coverage = content_coverage(&outcome, Some(&scope));
        let unjudged = outcome.prepare_unjudged();
        Self {
            outcome,
            scope,
            coverage,
            unjudged,
        }
    }
}

/// A complete, immutable verification outcome pinned by a reader.
pub struct PinnedResult {
    source: Id,
    version: Id,
    outcome: RegisteredOutcome,
    scope: Option<ScopeEvidence>,
    content_coverage: ContentCoverage,
}

impl PinnedResult {
    /// The scan result version from which the selected items came.
    pub fn source(&self) -> &Id {
        &self.source
    }
    /// The unique duplicate result version assigned by the caller.
    pub fn version(&self) -> &Id {
        &self.version
    }
    /// Scope evidence when this result was published by a scoped producer.
    pub fn scope(&self) -> Option<&ScopeEvidence> {
        self.scope.as_ref()
    }
    /// Content disposition coverage, separate from the job terminal state.
    pub fn content_coverage(&self) -> ContentCoverage {
        self.content_coverage
    }
    /// Registered strategy evidence; this is not an execution authorization.
    pub fn outcome(&self) -> &RegisteredOutcome {
        &self.outcome
    }
}

/// Admission or lookup failure. Transport adapters translate these at their boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The scan source is not registered as live.
    SourceUnavailable,
    /// The result identifier was already used in this session.
    DuplicateVersion,
    /// No result was ever published under this identifier.
    NotFound,
    /// The result belonged to a retired source.
    Stale,
    /// An explicitly configured live-body or tracked-identifier limit was reached.
    Capacity,
    /// Internal synchronization failed.
    Unavailable,
}

#[derive(Default)]
struct State {
    sources: HashSet<Id>,
    retired_sources: HashSet<Id>,
    versions: HashMap<Id, Option<Arc<PinnedResult>>>,
    latest: Option<Id>,
    order: Vec<Id>,
    by_source: HashMap<Id, HashSet<Id>>,
}

/// Session registry whose publication and source retirement are serialized.
///
/// Result data are released when their source is retired and the last admitted
/// reader drops its pin. Tombstones are kept for the session to distinguish stale
/// identifiers from unknown identifiers and to prevent accidental identifier reuse.
/// Limits bound registry-owned live bodies and all source and result identifiers,
/// including tombstones. Pins already admitted by readers may keep retired bodies
/// alive beyond the registry's live-body limit until those readers release them.
pub struct ResultStore {
    state: Mutex<State>,
    live_limit: NonZeroUsize,
    identifier_limit: NonZeroUsize,
}

impl ResultStore {
    /// Creates a registry with caller-selected, nonzero admission limits.
    ///
    /// No product retention policy or automatic eviction is selected here.
    pub fn new(live_limit: NonZeroUsize, identifier_limit: NonZeroUsize) -> Self {
        Self {
            state: Mutex::new(State::default()),
            live_limit,
            identifier_limit,
        }
    }

    /// Registers a live source before publishing any result derived from it.
    ///
    /// # Errors
    /// Returns `SourceUnavailable` if this source was retired; a source cannot be
    /// revived in the same session. A new source consumes one tracked identifier;
    /// re-registering a live source is idempotent. Returns `Capacity` if no
    /// identifier slot remains, or `Unavailable` on a poisoned registry.
    pub fn register_source(&self, source: Id) -> Result<(), StoreError> {
        let mut state = self.state.lock().map_err(|_| StoreError::Unavailable)?;
        if state.retired_sources.contains(&source) {
            return Err(StoreError::SourceUnavailable);
        }
        if state.sources.contains(&source) {
            return Ok(());
        }
        if tracked_identifiers(&state) >= self.identifier_limit.get() {
            return Err(StoreError::Capacity);
        }
        state.sources.insert(source);
        Ok(())
    }

    /// Publishes a newly completed registered outcome and atomically changes latest.
    ///
    /// # Errors
    /// Returns `SourceUnavailable` for an unknown or retired source,
    /// `DuplicateVersion` for any previously used result identifier, or
    /// `Capacity` when either limit is reached, or `Unavailable` on a poisoned
    /// registry. Failed publication changes nothing.
    pub fn publish(
        &self,
        source: &Id,
        version: Id,
        outcome: RegisteredOutcome,
    ) -> Result<(), StoreError> {
        let coverage = content_coverage(&outcome, None);
        self.publish_inner(source, version, outcome, None, coverage)
    }

    /// Publishes a candidate whose coverage and cancellation list are precomputed.
    pub(crate) fn publish_prepared(
        &self,
        source: &Id,
        version: Id,
        mut prepared: PreparedResult,
        cancelled: bool,
    ) -> Result<(), StoreError> {
        if cancelled {
            prepared.outcome.mark_cancelled_prepared(prepared.unjudged);
            prepared.coverage = ContentCoverage::Cancelled;
        }
        self.publish_inner(
            source,
            version,
            prepared.outcome,
            Some(prepared.scope),
            prepared.coverage,
        )
    }

    fn publish_inner(
        &self,
        source: &Id,
        version: Id,
        outcome: RegisteredOutcome,
        scope: Option<ScopeEvidence>,
        content_coverage: ContentCoverage,
    ) -> Result<(), StoreError> {
        let mut state = self.state.lock().map_err(|_| StoreError::Unavailable)?;
        if !state.sources.contains(source) {
            return Err(StoreError::SourceUnavailable);
        }
        if state.versions.contains_key(&version) {
            return Err(StoreError::DuplicateVersion);
        }
        if state.order.len() >= self.live_limit.get()
            || tracked_identifiers(&state) >= self.identifier_limit.get()
        {
            return Err(StoreError::Capacity);
        }
        let result = Arc::new(PinnedResult {
            source: source.clone(),
            version: version.clone(),
            outcome,
            scope,
            content_coverage,
        });
        state.versions.insert(version.clone(), Some(result));
        state
            .by_source
            .entry(source.clone())
            .or_default()
            .insert(version.clone());
        state.order.push(version.clone());
        state.latest = Some(version);
        Ok(())
    }

    /// Pins a specified result or the latest result, then reads require no store lock.
    ///
    /// # Errors
    /// Returns `NotFound` for an unknown identifier or absent latest result,
    /// `Stale` for a retired result, or `Unavailable` on a poisoned registry.
    pub fn pin(&self, version: Option<&Id>) -> Result<Arc<PinnedResult>, StoreError> {
        let state = self.state.lock().map_err(|_| StoreError::Unavailable)?;
        let key = version
            .or(state.latest.as_ref())
            .ok_or(StoreError::NotFound)?;
        state
            .versions
            .get(key)
            .ok_or(StoreError::NotFound)?
            .as_ref()
            .cloned()
            .ok_or(StoreError::Stale)
    }

    /// Retires a source and prevents new pins or publications for its results.
    /// Existing pins remain readable until their holders release them.
    ///
    /// # Errors
    /// Returns `SourceUnavailable` for an unknown or already retired source,
    /// or `Unavailable` on a poisoned registry.
    pub fn retire_source(&self, source: &Id) -> Result<(), StoreError> {
        let mut state = self.state.lock().map_err(|_| StoreError::Unavailable)?;
        if !state.sources.remove(source) {
            return Err(StoreError::SourceUnavailable);
        }
        state.retired_sources.insert(source.clone());
        if let Some(versions) = state.by_source.remove(source) {
            for id in &versions {
                if let Some(value) = state.versions.get_mut(id) {
                    *value = None;
                }
            }
            state.order.retain(|id| !versions.contains(id));
            state.latest = state.order.last().cloned();
        }
        Ok(())
    }
}

fn tracked_identifiers(state: &State) -> usize {
    state.sources.len() + state.retired_sources.len() + state.versions.len()
}

fn content_coverage(outcome: &RegisteredOutcome, scope: Option<&ScopeEvidence>) -> ContentCoverage {
    use crate::verification::State;
    let evidence = outcome.evidence();
    if evidence.cancelled {
        return ContentCoverage::Cancelled;
    }
    if scope.is_none_or(|scope| {
        scope.source_completeness != ResultCompleteness::Complete || scope.unavailable != 0
    }) || evidence
        .states
        .values()
        .any(|state| !matches!(state, State::Verified | State::Distinct))
    {
        return ContentCoverage::Partial;
    }
    ContentCoverage::Complete
}
