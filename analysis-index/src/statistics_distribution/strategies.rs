//! Internal F26 distribution strategy selection and chunk combination.
//!
//! This is not the published statistics IPC or shared strategy contract. The
//! caller owns source pinning, selected rows, model registration and publication.
use crate::statistics_distribution::{self as distribution, Amounts, Bins, Row};
use crate::{ResultVersion, statistics_summary};
use du_core_contracts::ItemRef;
use du_core_contracts::SizeBasis;
use std::collections::BTreeMap;

/// Distribution and original F03 evidence for one pinned subtree.
///
/// Bin totals describe observed file rows and are not a deduplicated F03 set
/// total. Unknown kinds and untraversed links remain visible in the summary.
pub struct SubtreeBins {
    pub summary: statistics_summary::Summary,
    pub computed: Computed,
}

/// Failure while collecting or computing a pinned subtree distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtreeError {
    Summary(statistics_summary::Error),
    Strategy(Error),
}

/// Computes registered bins from actual observed files of a pinned subtree.
///
/// The caller supplies boundaries and chunk size. The same pinned source also
/// supplies the untouched F03 aggregate and completeness evidence. Missing
/// per-file sizes remain unknown, including unavailable unique physical sizes.
/// Traversal is stable for the pinned tree but is not a platform path comparison
/// order. File identifiers are retained once; only one chunk of rows is built
/// at a time, with cancellation checked during collection and materialization.
pub fn subtree_bins(
    version: &ResultVersion,
    scope: &ItemRef,
    basis: &SizeBasis,
    boundaries: &[u64],
    chunk_size: usize,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<SubtreeBins, SubtreeError> {
    if chunk_size == 0 {
        return Err(SubtreeError::Strategy(Error::InvalidChunkSize));
    }
    let (summary, files) = statistics_summary::summarize_with_files(version, scope, cancelled)
        .map_err(SubtreeError::Summary)?;
    let registry = bins_registry().map_err(SubtreeError::Strategy)?;
    let key = Key {
        id: "stat.bins".into(),
        revision: 1,
    };
    let strategy = registry
        .0
        .get(&key)
        .ok_or(SubtreeError::Strategy(Error::UnknownStrategy))?;
    let mut partial = strategy
        .calculate(&[], basis, boundaries, cancelled)
        .map_err(SubtreeError::Strategy)?;
    for chunk in files.chunks(chunk_size) {
        let mut rows = Vec::new();
        for item in chunk {
            if cancelled() {
                return Err(SubtreeError::Strategy(Error::Cancelled));
            }
            rows.push(Row {
                size: version.item_size(*item, basis.clone()),
                category: distribution::Category::Missing,
            });
        }
        let next = strategy
            .calculate(&rows, basis, boundaries, cancelled)
            .map_err(SubtreeError::Strategy)?;
        if cancelled() {
            return Err(SubtreeError::Strategy(Error::Cancelled));
        }
        partial = strategy
            .combine(partial, next)
            .map_err(SubtreeError::Strategy)?;
    }
    if cancelled() {
        return Err(SubtreeError::Strategy(Error::Cancelled));
    }
    let computed = Computed {
        strategy: key,
        bins: partial.bins,
    };
    Ok(SubtreeBins { summary, computed })
}

/// Internal strategy identity, selected independently of the public contract.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub id: String,
    pub revision: u32,
}

/// A homogeneous bins computation over one caller-pinned selected set.
pub struct Request<'a> {
    pub rows: &'a [Row<'a>],
    pub basis: &'a SizeBasis,
    pub boundaries: &'a [u64],
    /// Explicit partition size; no default or measured threshold is assumed.
    pub chunk_size: usize,
}

/// Internal selection, configuration, calculation and cancellation failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidRegistration,
    DuplicateRegistration,
    UnknownStrategy,
    InvalidChunkSize,
    IncompatiblePartial,
    Calculation(distribution::Error),
    Cancelled,
}

/// A complete calculation with the exact selected identity; not a cached model.
pub struct Computed {
    pub strategy: Key,
    pub bins: Bins,
}

/// A partial aggregate retains the basis and exact interval boundaries it used.
pub struct Partial {
    pub basis: SizeBasis,
    pub boundaries: Vec<u64>,
    pub bins: Bins,
}

/// Typed chunk computation and associative partial combination for distributions.
pub trait Strategy: Send + Sync {
    fn calculate(
        &self,
        rows: &[Row<'_>],
        basis: &SizeBasis,
        boundaries: &[u64],
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Partial, Error>;
    fn combine(&self, left: Partial, right: Partial) -> Result<Partial, Error>;
}

/// Reuses the F26 bins assignment core and preserves its value-state counts.
pub struct SizeBins;
impl Strategy for SizeBins {
    fn calculate(
        &self,
        rows: &[Row<'_>],
        basis: &SizeBasis,
        boundaries: &[u64],
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Partial, Error> {
        let bins =
            distribution::bins(rows, basis, boundaries, cancelled).map_err(
                |error| match error {
                    distribution::Error::Cancelled => Error::Cancelled,
                    other => Error::Calculation(other),
                },
            )?;
        Ok(Partial {
            basis: basis.clone(),
            boundaries: boundaries.to_vec(),
            bins,
        })
    }
    fn combine(&self, mut left: Partial, right: Partial) -> Result<Partial, Error> {
        if left.basis != right.basis
            || left.boundaries != right.boundaries
            || left.bins.intervals.len() != right.bins.intervals.len()
        {
            return Err(Error::IncompatiblePartial);
        }
        for (a, b) in left.bins.intervals.iter_mut().zip(right.bins.intervals) {
            merge(a, b)?;
        }
        merge(&mut left.bins.outside, right.bins.outside)?;
        merge(&mut left.bins.unknown, right.bins.unknown)?;
        Ok(left)
    }
}

fn merge(a: &mut Amounts, b: Amounts) -> Result<(), Error> {
    let overflow = Error::Calculation(distribution::Error::Overflow);
    a.count = a.count.checked_add(b.count).ok_or(overflow)?;
    a.known_bytes = a.known_bytes.checked_add(b.known_bytes).ok_or(overflow)?;
    a.unknown_count = a
        .unknown_count
        .checked_add(b.unknown_count)
        .ok_or(overflow)?;
    a.lower_bound_count = a
        .lower_bound_count
        .checked_add(b.lower_bound_count)
        .ok_or(overflow)?;
    Ok(())
}

/// Composition-owned versioned registry; unknown selection never invokes a strategy.
#[derive(Default)]
pub struct Registry(BTreeMap<Key, Box<dyn Strategy>>);
impl Registry {
    /// Registers an identity once without replacing an existing strategy.
    pub fn register(&mut self, key: Key, strategy: Box<dyn Strategy>) -> Result<(), Error> {
        if key.id.trim().is_empty() || key.revision == 0 {
            return Err(Error::InvalidRegistration);
        }
        if self.0.contains_key(&key) {
            return Err(Error::DuplicateRegistration);
        }
        self.0.insert(key, strategy);
        Ok(())
    }

    /// Selects the exact revision or, when omitted, the highest revision of the ID.
    /// Cancellation discards the result; this function does not publish a model.
    pub fn compute(
        &self,
        id: &str,
        revision: Option<u32>,
        request: Request<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Computed, Error> {
        let (key, strategy) = match revision {
            Some(revision) => self.0.get_key_value(&Key {
                id: id.to_owned(),
                revision,
            }),
            None => self.0.iter().rev().find(|(key, _)| key.id == id),
        }
        .ok_or(Error::UnknownStrategy)?;
        if request.chunk_size == 0 {
            return Err(Error::InvalidChunkSize);
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut partial = strategy.calculate(&[], request.basis, request.boundaries, cancelled)?;
        for chunk in request.rows.chunks(request.chunk_size) {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let next = strategy.calculate(chunk, request.basis, request.boundaries, cancelled)?;
            if cancelled() {
                return Err(Error::Cancelled);
            }
            partial = strategy.combine(partial, next)?;
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(Computed {
            strategy: key.clone(),
            bins: partial.bins,
        })
    }
}

/// Constructs the currently implemented bins-only registration, not the full bundle.
pub fn bins_registry() -> Result<Registry, Error> {
    let mut registry = Registry::default();
    registry.register(
        Key {
            id: "stat.bins".into(),
            revision: 1,
        },
        Box::new(SizeBins),
    )?;
    Ok(registry)
}
