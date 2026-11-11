//! Pinned file observations projected into an internal histogram.
//!
//! Boundaries and presentation policy remain supplied by their owners. This is
//! not a registered F06 strategy or a published visualization model.
use super::{
    geometry::{Rect, Scale, Weight},
    histogram::{self, Basis, Bin, Histogram, Item},
};
use crate::{ResultVersion, statistics_summary};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue, result::ResultCompleteness,
};

/// Scope, source basis and externally supplied histogram policy.
pub struct Request<'a> {
    pub version: &'a ResultVersion,
    pub scope: &'a ItemRef,
    pub size_basis: SizeBasis,
    pub bins: &'a [Bin],
    pub bounds: Rect,
    pub histogram_basis: Basis,
    pub scale: Scale,
}

/// F03 aggregate and coverage are independent of the histogram's observed sums.
/// The summary retains all source gaps, including gaps outside the selected root.
/// Histogram completeness retains any non-complete source state; a complete source
/// is downgraded only when the selected root has possible omissions.
pub struct Model {
    pub summary: statistics_summary::Summary,
    pub histogram: Histogram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Source(statistics_summary::Error),
    Layout(histogram::Error),
    InvalidSize,
    Cancelled,
}

/// Uses actual observed files of one pinned scope; directories are not added as files.
pub fn project(request: Request<'_>, cancelled: &mut dyn FnMut() -> bool) -> Result<Model, Error> {
    let (summary, files) =
        statistics_summary::summarize_with_files(request.version, request.scope, cancelled)
            .map_err(|e| {
                if e == statistics_summary::Error::Cancelled {
                    Error::Cancelled
                } else {
                    Error::Source(e)
                }
            })?;
    let mut items = Vec::with_capacity(files.len());
    for item_no in files {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        items.push(Item {
            item_no,
            value: weight(
                request
                    .version
                    .item_size(item_no, request.size_basis.clone()),
            )?,
        });
    }
    // Root identity is retained even when native path-comparison evidence is not.
    // A gap in this root may cover the scope, so do not discard it based on raw names.
    let root = request
        .version
        .root_indices
        .get(request.scope.item_no as usize)
        .ok_or(Error::Source(statistics_summary::Error::InvalidScope))?;
    let mut possible_gap = false;
    for gap in &summary.source_gaps {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if gap.root_index == *root {
            possible_gap = true;
            break;
        }
    }
    let omissions =
        summary.counts.unknown_kind > 0 || summary.counts.untraversed_links > 0 || possible_gap;
    let completeness = if summary.result_completeness == ResultCompleteness::Complete && omissions {
        ResultCompleteness::Partial
    } else {
        summary.result_completeness
    };
    let histogram = histogram::layout_cancellable(
        request.bounds,
        request.bins,
        &items,
        request.histogram_basis,
        request.scale,
        completeness,
        cancelled,
    )
    .map_err(|e| {
        if e == histogram::Error::Cancelled {
            Error::Cancelled
        } else {
            Error::Layout(e)
        }
    })?;
    Ok(Model { summary, histogram })
}

fn weight(value: Option<&SizeValue>) -> Result<Weight, Error> {
    let Some(value) = value else {
        return Ok(Weight::Unknown);
    };
    if value.evidence == SizeEvidence::Unknown {
        return Ok(Weight::Unknown);
    }
    match &value.amount {
        SizeAmount::Exact { bytes } => bytes
            .as_str()
            .parse()
            .map(Weight::Exact)
            .map_err(|_| Error::InvalidSize),
        SizeAmount::LowerBound { bytes } => bytes
            .as_str()
            .parse()
            .map(Weight::LowerBound)
            .map_err(|_| Error::InvalidSize),
        SizeAmount::Unknown => Ok(Weight::Unknown),
    }
}
