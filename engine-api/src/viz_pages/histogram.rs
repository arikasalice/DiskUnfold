//! Internal item-count histogram candidate and same-model group rows.
//!
//! This module does not register a strategy or a GUI command. Labels, legend
//! classes, group references, and missing-value reasons come from their owners.
use super::{ModelInput, production::RowPresentation};
use crate::browse;
use du_analysis_index::{
    ResultVersion,
    visualization::{
        geometry::{Rect, Scale, Weight},
        histogram::{Basis, Bin},
        histogram_projection,
        live_models::{GroupRows, SizeRange},
    },
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::ResultVersionRead,
    visualization::{
        quantity::VizQuantity,
        rows::{VizDenominator, VizDenominatorBasis, VizRow},
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Presentation and reference evidence for a single supplied bin.
pub struct BinPresentation {
    pub group_ref: String,
    pub display_name: String,
    pub legend_class_key: String,
    pub display_flags: Vec<String>,
}
/// All policies and item presentations are supplied for this calculation.
pub struct Request<'a> {
    pub version: Arc<ResultVersion>,
    pub scope: &'a ItemRef,
    pub size_basis: SizeBasis,
    pub bins: &'a [Bin],
    pub bounds: Rect,
    pub scale: Scale,
    pub fingerprint: String,
    pub bin_presentation: Vec<BinPresentation>,
    pub item_presentation: Vec<RowPresentation>,
    pub unknown_reason: String,
}
/// A private candidate; publish the rows through `ModelPages` only after admission.
pub struct Candidate {
    pub model: histogram_projection::Model,
    pub pages: ModelInput,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Source,
    MissingPresentation,
    InvalidQuantity,
    Cancelled,
}
impl From<super::histogram_fingerprint::Error> for Error {
    fn from(error: super::histogram_fingerprint::Error) -> Self {
        match error {
            super::histogram_fingerprint::Error::Cancelled => Self::Cancelled,
            super::histogram_fingerprint::Error::Source => Self::Source,
        }
    }
}

/// Computes the model and rows from one immutable source, without publishing either.
pub fn prepare(
    request: Request<'_>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Candidate, Error> {
    if request.fingerprint.is_empty()
        || request.unknown_reason.trim().is_empty()
        || request.bins.len() != request.bin_presentation.len()
    {
        return Err(Error::MissingPresentation);
    }
    let model = histogram_projection::project(
        histogram_projection::Request {
            version: &request.version,
            scope: request.scope,
            size_basis: request.size_basis.clone(),
            bins: request.bins,
            bounds: request.bounds,
            histogram_basis: Basis::ItemCount,
            scale: request.scale,
        },
        cancelled,
    )
    .map_err(|e| {
        if e == histogram_projection::Error::Cancelled {
            Error::Cancelled
        } else {
            Error::Source
        }
    })?;
    let count = model.summary.counts.files;
    let denominator = VizDenominator {
        basis: VizDenominatorBasis::Scope,
        value: if model.histogram.source_completeness
            == du_core_contracts::result::ResultCompleteness::Complete
        {
            VizQuantity::exact("items", i128::from(count))
        } else {
            VizQuantity::lower_bound("items", count)
        }
        .ok_or(Error::InvalidQuantity)?,
        size: None,
    };
    let mut presentations = collect_presentations(
        request.item_presentation,
        request.version.version_id(),
        cancelled,
    )?;
    let mut groups = HashMap::new();
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for (bin, p) in model.histogram.bins.iter().zip(request.bin_presentation) {
        check(cancelled)?;
        if p.group_ref.is_empty()
            || p.legend_class_key.is_empty()
            || !seen.insert(p.group_ref.clone())
        {
            return Err(Error::MissingPresentation);
        }
        let mut members = Vec::new();
        let mut refs = Vec::new();
        for item_no in &bin.members {
            check(cancelled)?;
            let p = presentations
                .remove(item_no)
                .ok_or(Error::MissingPresentation)?;
            let item = request
                .version
                .detail(&p.item)
                .map_err(|_| Error::Source)?
                .item;
            let (_, root) = request
                .version
                .observed_root_for(&p.item)
                .map_err(|_| Error::Source)?;
            let name = browse::display(&root.path.encoding, &item.name).ok_or(Error::Source)?;
            let size = request
                .version
                .item_size(*item_no, request.size_basis.clone())
                .cloned()
                .unwrap_or(SizeValue {
                    basis: request.size_basis.clone(),
                    evidence: SizeEvidence::Unknown,
                    amount: SizeAmount::Unknown,
                });
            let row = VizRow {
                item_ref: Some(p.item.clone()),
                group_ref: None,
                display_name: name,
                size,
                quantity: None,
                ratio: None,
                denominator: denominator.clone(),
                legend_class_key: p.legend_class_key,
                display_flags: p.display_flags,
            };
            row.validate_revision(request.version.version_id())
                .map_err(|_| Error::MissingPresentation)?;
            refs.push(p.item);
            members.push(row);
        }
        let complete = matches!(bin.item_count, Weight::Exact(_)) && !bin.count_overflowed;
        let aggregate = request
            .version
            .selection_total(&refs, complete, &mut *cancelled)
            .map_err(|e| {
                if e == du_analysis_index::query::SpaceTotalError::Cancelled {
                    Error::Cancelled
                } else {
                    Error::Source
                }
            })?;
        let size = match request.size_basis {
            SizeBasis::Logical => aggregate.logical,
            SizeBasis::Allocated => aggregate.allocated,
            SizeBasis::UniquePhysical => aggregate.unique_physical,
        };
        let quantity = match bin.item_count {
            Weight::Exact(n) => VizQuantity::exact("items", i128::from(n)),
            Weight::LowerBound(n) => VizQuantity::lower_bound("items", n),
            Weight::Unknown => VizQuantity::unknown("items", request.unknown_reason.clone()),
        }
        .ok_or(Error::InvalidQuantity)?;
        let row = VizRow {
            item_ref: None,
            group_ref: Some(p.group_ref.clone()),
            display_name: p.display_name,
            size: size.clone(),
            quantity: Some(quantity),
            ratio: None,
            denominator: denominator.clone(),
            legend_class_key: p.legend_class_key,
            display_flags: p.display_flags,
        };
        row.validate_revision(request.version.version_id())
            .map_err(|_| Error::MissingPresentation)?;
        groups.insert(
            p.group_ref,
            GroupRows {
                rows: members,
                total: size,
                size_range: SizeRange::Unknown,
            },
        );
        rows.push(row);
    }
    if !presentations.is_empty() {
        return Err(Error::MissingPresentation);
    }
    check(cancelled)?;
    let fingerprint = super::histogram_fingerprint::candidate_fingerprint(
        &request.fingerprint,
        &model.histogram,
        &rows,
        &groups,
        cancelled,
    )?;
    Ok(Candidate {
        model,
        pages: ModelInput {
            version: request.version,
            condition_fingerprint: fingerprint,
            rows,
            groups,
        },
    })
}
fn check(cancelled: &mut dyn FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
fn collect_presentations(
    input: Vec<RowPresentation>,
    revision: &du_core_contracts::Id,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<HashMap<u32, RowPresentation>, Error> {
    let mut output = HashMap::new();
    for p in input {
        check(cancelled)?;
        if &p.item.result_version != revision
            || p.legend_class_key.is_empty()
            || output.insert(p.item.item_no, p).is_some()
        {
            return Err(Error::MissingPresentation);
        }
    }
    Ok(output)
}
