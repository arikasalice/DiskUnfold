//! Deferred histogram page registration for a caller-owned publication gate.
use super::{Error as HistogramError, SelectedCandidate};
use crate::viz_pages::{self, DeferredRows, LayoutMetadata, ModelPages, PreparedModelInput};
use du_analysis_index::visualization::{
    geometry::{NoArea, Rect},
    histogram::{Basis, Bin},
    unit_bins::UnitBins,
};
use du_core_contracts::result::ResultVersionRead;
use std::sync::{Arc, Weak};

/// One immutable geometry row associated with a model-local group reference.
pub struct HistogramBinFrame {
    pub group_ref: String,
    pub bin: Bin,
    pub rect: Option<Rect>,
    pub no_area: Option<NoArea>,
}

/// Bounded geometry and table evidence; member rows live in the pinned source.
pub struct HistogramFrame {
    /// Quantity represented by the axis and each bin geometry.
    pub basis: Basis,
    pub unit_bins: UnitBins,
    pub axis_max: Option<u64>,
    pub bins: Vec<HistogramBinFrame>,
}

/// One reserved registration, including cleanup retained outside the outer gate.
pub struct DeferredSelectedPages {
    owner: Weak<()>,
    input: PreparedModelInput,
    reference: String,
    cleanup: DeferredRows,
    used: bool,
    frame: LayoutMetadata,
}
impl DeferredSelectedPages {
    /// Returns the reserved model reference; it is not readable until commit succeeds.
    pub fn reserved_reference(&self) -> &str {
        &self.reference
    }
}

impl ModelPages {
    /// Reads geometry only if its model and source are still admitted together.
    pub fn histogram_frame(
        &self,
        reference: &str,
    ) -> Result<Arc<HistogramFrame>, viz_pages::Error> {
        self.layout_metadata(reference)
    }

    /// Validates the candidate and reserves its reference before entering a job gate.
    pub fn defer_histogram_pages(
        &self,
        candidate: SelectedCandidate,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<DeferredSelectedPages, HistogramError> {
        if cancelled() {
            return Err(HistogramError::Cancelled);
        }
        let candidate = candidate.0;
        let unit_bins = candidate.unit_bins.ok_or(HistogramError::Source)?;
        if unit_bins.bins.len() != candidate.model.histogram.bins.len()
            || candidate.pages.rows.len() != unit_bins.bins.len()
        {
            return Err(HistogramError::Source);
        }
        if candidate.pages.groups.len() != unit_bins.bins.len() {
            return Err(HistogramError::Source);
        }
        let mut seen = std::collections::HashSet::new();
        let mut bins = Vec::with_capacity(unit_bins.bins.len());
        for ((bin, boundary), row) in candidate
            .model
            .histogram
            .bins
            .iter()
            .zip(&unit_bins.bins)
            .zip(&candidate.pages.rows)
        {
            if cancelled() {
                return Err(HistogramError::Cancelled);
            }
            if bin.bin != *boundary {
                return Err(HistogramError::Source);
            }
            let group_ref = row
                .group_ref
                .clone()
                .filter(|key| candidate.pages.groups.contains_key(key))
                .ok_or(HistogramError::Source)?;
            if !seen.insert(group_ref.clone()) {
                return Err(HistogramError::Source);
            }
            bins.push(HistogramBinFrame {
                group_ref,
                bin: bin.bin,
                rect: bin.rect,
                no_area: bin.no_area,
            });
        }
        let frame = Arc::new(HistogramFrame {
            basis: candidate.model.histogram.basis,
            unit_bins,
            axis_max: candidate.model.histogram.axis_max,
            bins,
        });
        let scope = candidate.model.summary.scope;
        if scope.result_version != *candidate.pages.version.version_id()
            || candidate.model.summary.result_version != *candidate.pages.version.version_id()
        {
            return Err(HistogramError::Source);
        }
        let pages = candidate.pages;
        if pages.condition_fingerprint.is_empty() || pages.groups.keys().any(String::is_empty) {
            return Err(HistogramError::Source);
        }
        validate(&pages, &pages.rows, cancelled)?;
        for group in pages.groups.values() {
            validate(&pages, &group.rows, cancelled)?;
        }
        if cancelled() {
            return Err(HistogramError::Cancelled);
        }
        let input = PreparedModelInput {
            version: pages.version,
            condition_fingerprint: pages.condition_fingerprint,
            rows: Some(
                du_analysis_index::visualization::live_models::PreparedRows::new(
                    pages.rows,
                    pages.groups,
                ),
            ),
            scope: Some(scope),
        };
        Ok(DeferredSelectedPages {
            owner: Arc::downgrade(&self.identity),
            input,
            reference: self.ids.next_id().as_str().to_owned(),
            cleanup: DeferredRows::default(),
            used: false,
            frame,
        })
    }
    /// Commits one reserved candidate while retaining retired rows in its owner.
    /// The caller must keep `deferred` alive until after the outer gate is released.
    pub fn commit_histogram_pages(
        &self,
        deferred: &mut DeferredSelectedPages,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<String, viz_pages::Error> {
        if deferred.used {
            return Err(viz_pages::Error::InvalidModel);
        }
        deferred.used = true;
        if !deferred
            .owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &self.identity))
        {
            return Err(viz_pages::Error::InvalidModel);
        }
        let (reference, _) = self.register_deferred(
            &mut deferred.input,
            &(None, Some(deferred.frame.clone())),
            &deferred.reference,
            cancelled,
            &mut deferred.cleanup,
        )?;
        Ok(reference)
    }
}

fn validate(
    input: &viz_pages::ModelInput,
    rows: &[du_core_contracts::visualization::rows::VizRow],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), HistogramError> {
    viz_pages::validate_rows_cancellable(input, rows, cancelled).map_err(|error| {
        if error == viz_pages::Error::Cancelled {
            HistogramError::Cancelled
        } else {
            HistogramError::Source
        }
    })
}
