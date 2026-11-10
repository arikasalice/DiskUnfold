//! Page an already admitted live model group without retaining an index lock.
//!
//! The session owner must first atomically admit the model's source as the
//! current live version. This module pins only the exact supplied source and
//! its group, and does not decide IPC error precedence or model membership.
use super::live_models::{self, GroupRows, SizeRange};
use crate::ResultVersion;
use du_core_contracts::{
    Id, SizeValue, Timestamp,
    result::{ResultCompleteness, ResultVersionRead},
    visualization::rows::{MAX_PAGE_COUNT, VizRow},
};
use std::sync::Arc;

/// An admitted source and its model-local, immutable group rows.
pub struct PinnedGroup {
    source: Arc<ResultVersion>,
    group: Arc<GroupRows>,
}

/// Source-owned failures during pinning, paging, or cancellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    ModelUnavailable,
    ModelExpired,
    GroupMissing,
    InvalidRange,
    Cancelled,
}

/// A complete page with the original group's value and range evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub rows: Vec<VizRow>,
    pub total_count: u64,
    pub group_total: SizeValue,
    pub size_range: SizeRange,
    pub result_revision: Id,
    pub observed_at: Option<Timestamp>,
    pub completeness: ResultCompleteness,
}

impl PinnedGroup {
    /// Pins one group after the session owner admits this exact current source.
    ///
    /// This does not check whether the supplied version is currently published;
    /// the caller must perform that admission and hold the returned source pin.
    /// Once admitted, source replacement or model eviction cannot alter this pin.
    pub fn from_admitted(
        source: Arc<ResultVersion>,
        model_ref: &str,
        group_ref: &str,
    ) -> Result<Self, Error> {
        let group = source
            .pin_viz_group(model_ref, group_ref)
            .map_err(|error| match error {
                live_models::Error::Unavailable | live_models::Error::Duplicate => {
                    Error::ModelUnavailable
                }
                live_models::Error::Missing => Error::ModelExpired,
            })?
            .ok_or(Error::GroupMissing)?;
        Ok(Self { source, group })
    }

    /// Returns the fixed member count without copying rows or taking another lock.
    pub fn row_count(&self) -> usize {
        self.group.rows.len()
    }

    /// Copies a bounded page in producer order, discarding partial work on cancellation.
    ///
    /// `start == total_count` is a valid empty page. The count must be 1..=1000.
    /// No total, size range, value state, or grouping is recalculated.
    pub fn page(
        &self,
        start: u32,
        count: u32,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Page, Error> {
        if count == 0 || count > MAX_PAGE_COUNT || start as usize > self.row_count() {
            return Err(Error::InvalidRange);
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let start = start as usize;
        let end = start
            .saturating_add(count as usize)
            .min(self.group.rows.len());
        let mut rows = Vec::with_capacity(end - start);
        for row in &self.group.rows[start..end] {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            rows.push(row.clone());
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(Page {
            rows,
            total_count: self.group.rows.len() as u64,
            group_total: self.group.total.clone(),
            size_range: self.group.size_range.clone(),
            result_revision: self.source.version_id().clone(),
            observed_at: self.source.observed_at().cloned(),
            completeness: self.source.completeness(),
        })
    }
}
