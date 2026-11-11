//! Internal histogram candidate and same-model group rows.
//!
//! This module does not register a strategy or a GUI command. Labels, legend
//! classes, group references, and missing-value reasons come from their owners.
mod deferred_selected;
mod selected;
use super::{ModelInput, production::RowPresentation};
use crate::browse;
pub use deferred_selected::{
    DeferredSelectedPages, HistogramBinFrame, HistogramFrame, HistogramStatus,
};
use du_analysis_index::{
    ResultVersion,
    visualization::{
        geometry::{Rect, Scale, Weight},
        histogram::{Basis, Bin},
        histogram_projection,
        live_models::{GroupRows, SizeRange},
        ratio,
    },
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::ResultVersionRead,
    visualization::{
        quantity::VizQuantity,
        rows::{VizDenominator, VizDenominatorBasis, VizRatio, VizRow},
    },
};
pub use selected::{PinnedRendered, SelectedCandidate, prepare_selected};
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
    pub unit_bins: Option<du_analysis_index::visualization::unit_bins::UnitBins>,
    pub no_attribution: bool,
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
    assemble(request, model, cancelled)
}

fn assemble(
    request: Request<'_>,
    mut model: histogram_projection::Model,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Candidate, Error> {
    let denominator = denominator(&request, &model)?;
    let mut presentations = collect_presentations(
        request.item_presentation,
        request.version.version_id(),
        cancelled,
    )?;
    let mut groups = HashMap::new();
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    let mut no_attribution = false;
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
            let item_ratio = if model.histogram.basis == Basis::Bytes {
                let value = quantity("bytes", size_weight(&size)?, &request.unknown_reason)?;
                byte_ratio(&value, &denominator)?
            } else {
                None
            };
            let row = VizRow {
                item_ref: Some(p.item.clone()),
                group_ref: None,
                display_name: name,
                size,
                quantity: None,
                ratio: item_ratio,
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
        // A bin's drawn byte amount must agree with F03's independently
        // calculated set total; overlapping or unsupported attribution fails closed.
        let source_weight = size_weight(&size)?;
        no_attribution |= model.histogram.basis == Basis::Bytes && source_weight != bin.bytes;
        let (unit, weight) = match model.histogram.basis {
            Basis::ItemCount => ("items", bin.item_count),
            Basis::Bytes => ("bytes", source_weight),
        };
        let quantity = quantity(unit, weight, &request.unknown_reason)?;
        let row_ratio = if model.histogram.basis == Basis::Bytes {
            byte_ratio(&quantity, &denominator)?
        } else {
            None
        };
        let row = VizRow {
            item_ref: None,
            group_ref: Some(p.group_ref.clone()),
            display_name: p.display_name,
            size: size.clone(),
            quantity: Some(quantity),
            ratio: row_ratio,
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
    if no_attribution {
        model.histogram.axis_max = None;
        for bin in &mut model.histogram.bins {
            bin.rect = None;
            bin.no_area = None;
        }
    }
    check(cancelled)?;
    let fingerprint = super::histogram_fingerprint::candidate_fingerprint(
        &request.fingerprint,
        &model.histogram,
        &rows,
        &groups,
        no_attribution,
        cancelled,
    )?;
    Ok(Candidate {
        model,
        unit_bins: None,
        no_attribution,
        pages: ModelInput {
            version: request.version,
            condition_fingerprint: fingerprint,
            rows,
            groups,
        },
    })
}
fn quantity(unit: &str, weight: Weight, unknown_reason: &str) -> Result<VizQuantity, Error> {
    match weight {
        Weight::Exact(n) => VizQuantity::exact(unit, i128::from(n)),
        Weight::LowerBound(n) => VizQuantity::lower_bound(unit, n),
        Weight::Unknown => VizQuantity::unknown(unit, unknown_reason.to_owned()),
    }
    .ok_or(Error::InvalidQuantity)
}

fn byte_ratio(
    value: &VizQuantity,
    denominator: &VizDenominator,
) -> Result<Option<VizRatio>, Error> {
    ratio::calculate(value, &denominator.value)
        .as_deref()
        .map(VizRatio::parse)
        .transpose()
        .map_err(|_| Error::InvalidQuantity)
}

fn size_weight(size: &SizeValue) -> Result<Weight, Error> {
    match &size.amount {
        SizeAmount::Exact { bytes } => bytes
            .as_str()
            .parse()
            .map(Weight::Exact)
            .map_err(|_| Error::InvalidQuantity),
        SizeAmount::LowerBound { bytes } => bytes
            .as_str()
            .parse()
            .map(Weight::LowerBound)
            .map_err(|_| Error::InvalidQuantity),
        SizeAmount::Unknown => Ok(Weight::Unknown),
    }
}

fn denominator(
    request: &Request<'_>,
    model: &histogram_projection::Model,
) -> Result<VizDenominator, Error> {
    let (value, size) = match model.histogram.basis {
        Basis::ItemCount => {
            let count = model.summary.counts.files;
            let weight = if model.histogram.source_completeness
                == du_core_contracts::result::ResultCompleteness::Complete
            {
                Weight::Exact(count)
            } else {
                Weight::LowerBound(count)
            };
            (quantity("items", weight, &request.unknown_reason)?, None)
        }
        Basis::Bytes => {
            let source = match request.size_basis {
                SizeBasis::Logical => &model.summary.aggregate.logical,
                SizeBasis::Allocated => &model.summary.aggregate.allocated,
                SizeBasis::UniquePhysical => &model.summary.aggregate.unique_physical,
            };
            let weight = size_weight(source)?;
            (
                quantity("bytes", weight, &request.unknown_reason)?,
                Some(source.clone()),
            )
        }
    };
    Ok(VizDenominator {
        basis: VizDenominatorBasis::Scope,
        value,
        size,
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
