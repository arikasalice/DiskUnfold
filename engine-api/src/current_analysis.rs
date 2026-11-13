//! Admission of a caller-selected current-analysis revision.
//!
//! The caller must select its own scope before invoking this boundary. It does
//! not interpret a source reference or choose the store's globally newest
//! result. Published versions without a bound analysis lineage are not evidence
//! of a current-analysis source.
use std::sync::Arc;

use du_analysis_index::{IngestSink, QueryError, ResultVersion};
use du_core_contracts::{Id, result::ResultVersionRead};

/// An immutable current-analysis result and the identifiers retained by its producer.
#[derive(Clone)]
pub struct CurrentAnalysis {
    version: Arc<ResultVersion>,
    lineage: Id,
}

impl CurrentAnalysis {
    pub fn version(&self) -> &Arc<ResultVersion> {
        &self.version
    }
    pub fn result_version(&self) -> &Id {
        self.version.version_id()
    }
    pub fn scope_version(&self) -> &Id {
        self.version.scope_version_id()
    }
    pub fn lineage(&self) -> &Id {
        &self.lineage
    }
}

/// Failures are internal; transport and reason-key mapping belong to the use case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NoSource,
    StaleVersion,
    UnsupportedSource,
    Unavailable,
}

/// Pins the caller's previously selected current-analysis revision once.
///
/// `selected` is a result revision, not a scope, lineage, job, or source ref.
/// An optional requested revision must match it. No result is silently substituted.
/// The returned Arc remains valid for the admitted calculation after replacement.
pub fn admit_selected(
    source: &IngestSink,
    selected: &Id,
    requested_revision: Option<&Id>,
) -> Result<CurrentAnalysis, Error> {
    if requested_revision.is_some_and(|revision| revision != selected) {
        return Err(Error::StaleVersion);
    }
    let version = source.pin_current(selected).map_err(pin_error)?;
    bind_lineage(version)
}

/// Admits a visualization request against the current scan result store.
/// An explicit revision is never substituted with a newer one.
pub(crate) fn admit_scan(
    source: &IngestSink,
    lineage: Option<&Id>,
    revision: Option<&Id>,
) -> Result<CurrentAnalysis, Error> {
    bind_lineage(source.pin_scan(lineage, revision).map_err(pin_error)?)
}

fn pin_error(error: QueryError) -> Error {
    match error {
        QueryError::NotFound => Error::NoSource,
        QueryError::StaleVersion => Error::StaleVersion,
        _ => Error::Unavailable,
    }
}

fn bind_lineage(version: Arc<ResultVersion>) -> Result<CurrentAnalysis, Error> {
    let lineage = version
        .lineage_id()
        .cloned()
        .ok_or(Error::UnsupportedSource)?;
    Ok(CurrentAnalysis { version, lineage })
}
