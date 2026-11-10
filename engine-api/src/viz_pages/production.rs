//! Assembly of admitted single-parent projections into pinned page rows.
//! Legend classifications and display flags must come from their owning producer.
use super::{Error as PagesError, ModelInput, ModelPages};
use crate::browse;
use du_analysis_index::{
    ResultVersion,
    visualization::{projection, ratio},
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::ResultVersionRead,
    visualization::{
        quantity::VizQuantity,
        rows::{VizDenominator, VizDenominatorBasis, VizRatio, VizRow},
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Classification and flags already evaluated by their owning producer for one item.
pub struct RowPresentation {
    pub item: ItemRef,
    pub legend_class_key: String,
    pub display_flags: Vec<String>,
}
/// Assembly refuses inputs when required source or presentation evidence is unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    SourceMismatch,
    MissingDisplayName,
    MissingClassification,
    InvalidQuantity,
    InvalidRatio,
    UnsupportedGroups,
    Cancelled,
    Pages(PagesError),
}

/// Registers rows of one ungrouped scope-denominator projection in the live model store.
///
/// The caller keeps `model` for geometry and supplies the same canonical fingerprint
/// used for the layout. Classifications and flags must come from the pinned model's
/// owning producers; this function does not classify, synthesize groups, or do I/O.
pub fn register_projection(
    pages: &ModelPages,
    version: Arc<ResultVersion>,
    model: &projection::Model,
    fingerprint: String,
    presentations: Vec<RowPresentation>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<String, Error> {
    let input = project_rows(version, model, fingerprint, presentations, cancelled)?;
    pages.register(input).map_err(Error::Pages)
}

pub(super) fn project_rows(
    version: Arc<ResultVersion>,
    model: &projection::Model,
    fingerprint: String,
    presentations: Vec<RowPresentation>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<ModelInput, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    validate_model(&version, model)?;
    if model.rows.len() != presentations.len() {
        return Err(Error::MissingClassification);
    }
    let denominator = denominator(&model.totals.scope, &model.basis)?;
    let mut by_item = presentations_by_item(&model.result_revision, presentations, cancelled)?;
    let mut rows = Vec::with_capacity(model.rows.len());
    for row in &model.rows {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let presentation = by_item
            .remove(&row.item.item_no)
            .ok_or(Error::MissingClassification)?;
        rows.push(project_row(
            &version,
            model,
            row,
            presentation,
            &denominator,
        )?);
    }
    if !by_item.is_empty() {
        return Err(Error::MissingClassification);
    }
    let mut seen = HashSet::new();
    if model
        .shapes
        .iter()
        .any(|s| s.row_index >= rows.len() || !seen.insert(s.row_index))
    {
        return Err(Error::SourceMismatch);
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(ModelInput {
        version,
        condition_fingerprint: fingerprint,
        rows,
        groups: HashMap::new(),
    })
}

/// Builds a table from the original admitted source without creating geometry.
pub(super) fn project_fallback(
    version: Arc<ResultVersion>,
    parent: &ItemRef,
    basis: &SizeBasis,
    fingerprint: String,
    presentations: Vec<RowPresentation>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<ModelInput, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let folder = du_analysis_index::browse::open(&version, Some(parent))
        .map_err(|_| Error::SourceMismatch)?;
    if folder.children.len() != presentations.len() {
        return Err(Error::MissingClassification);
    }
    let total = original_size(&version, parent.item_no, basis).ok_or(Error::SourceMismatch)?;
    let denominator = denominator(total, basis)?;
    let mut by_item = presentations_by_item(version.version_id(), presentations, cancelled)?;
    let mut rows = Vec::with_capacity(folder.children.len());
    for item in folder.children {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let presentation = by_item
            .remove(&item.item_no)
            .ok_or(Error::MissingClassification)?;
        let size = original_size(&version, item.item_no, basis).ok_or(Error::SourceMismatch)?;
        rows.push(make_row(
            &version,
            &item.name,
            size,
            presentation,
            &denominator,
        )?);
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(ModelInput {
        version,
        condition_fingerprint: fingerprint,
        rows,
        groups: HashMap::new(),
    })
}

fn presentations_by_item(
    revision: &du_core_contracts::Id,
    presentations: Vec<RowPresentation>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<HashMap<u32, RowPresentation>, Error> {
    let mut by_item = HashMap::new();
    for presentation in presentations {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if &presentation.item.result_version != revision
            || presentation.legend_class_key.is_empty()
            || by_item
                .insert(presentation.item.item_no, presentation)
                .is_some()
        {
            return Err(Error::MissingClassification);
        }
    }
    Ok(by_item)
}
fn validate_model(version: &ResultVersion, model: &projection::Model) -> Result<(), Error> {
    if model.result_revision != *version.version_id()
        || model.parent.result_version != model.result_revision
        || model.completeness != version.completeness()
        || model.observed_at.as_ref() != version.observed_at()
    {
        return Err(Error::SourceMismatch);
    }
    if model.totals.grouped_bytes != 0 {
        return Err(Error::UnsupportedGroups);
    }
    let original =
        original_size(version, model.parent.item_no, &model.basis).ok_or(Error::SourceMismatch)?;
    if original != &model.totals.scope
        || du_analysis_index::browse::open(version, Some(&model.parent))
            .map_err(|_| Error::SourceMismatch)?
            .children
            .len()
            != model.rows.len()
    {
        return Err(Error::SourceMismatch);
    }
    Ok(())
}
fn project_row(
    version: &ResultVersion,
    model: &projection::Model,
    row: &projection::Row,
    presentation: RowPresentation,
    denominator: &VizDenominator,
) -> Result<VizRow, Error> {
    let item = version
        .items()
        .get(row.item.item_no as usize)
        .ok_or(Error::SourceMismatch)?;
    if row.item.result_version != model.result_revision
        || item.parent != Some(model.parent.item_no)
        || original_size(version, item.item_no, &model.basis) != Some(&row.size)
    {
        return Err(Error::SourceMismatch);
    }
    let projected = make_row(version, &item.name, &row.size, presentation, denominator)?;
    if row.ratio.as_deref() != projected.ratio.as_ref().map(VizRatio::as_str) {
        return Err(Error::InvalidRatio);
    }
    Ok(projected)
}

fn make_row(
    version: &ResultVersion,
    name: &[u8],
    size: &SizeValue,
    presentation: RowPresentation,
    denominator: &VizDenominator,
) -> Result<VizRow, Error> {
    let (_, root) = version
        .observed_root_for(&presentation.item)
        .map_err(|_| Error::MissingDisplayName)?;
    let display_name =
        browse::display(&root.path.encoding, name).ok_or(Error::MissingDisplayName)?;
    let numerator = quantity(size)?;
    let ratio = ratio::calculate(&numerator, &denominator.value)
        .as_deref()
        .map(VizRatio::parse)
        .transpose()
        .map_err(|_| Error::InvalidRatio)?;
    let projected = VizRow {
        item_ref: Some(presentation.item),
        group_ref: None,
        display_name,
        size: size.clone(),
        quantity: None,
        ratio,
        denominator: denominator.clone(),
        legend_class_key: presentation.legend_class_key,
        display_flags: presentation.display_flags,
    };
    projected
        .validate_revision(version.version_id())
        .map_err(|_| Error::MissingClassification)?;
    Ok(projected)
}
fn denominator(size: &SizeValue, basis: &SizeBasis) -> Result<VizDenominator, Error> {
    if &size.basis != basis {
        return Err(Error::SourceMismatch);
    }
    Ok(VizDenominator {
        basis: VizDenominatorBasis::Scope,
        value: quantity(size)?,
        size: Some(size.clone()),
    })
}
fn original_size<'a>(
    version: &'a ResultVersion,
    item_no: u32,
    basis: &SizeBasis,
) -> Option<&'a SizeValue> {
    let scope = version.space(item_no)?;
    Some(match basis {
        SizeBasis::Logical => &scope.logical,
        SizeBasis::Allocated => &scope.allocated,
        SizeBasis::UniquePhysical => &scope.unique_physical,
    })
}

fn quantity(size: &SizeValue) -> Result<VizQuantity, Error> {
    let bytes = |bytes: &du_core_contracts::ByteSize| {
        bytes
            .as_str()
            .parse::<u64>()
            .map_err(|_| Error::InvalidQuantity)
    };
    let value = match &size.amount {
        SizeAmount::Exact { bytes: value } if size.evidence != SizeEvidence::Unknown => {
            VizQuantity::exact("bytes", i128::from(bytes(value)?))
        }
        SizeAmount::Exact { .. } => return Err(Error::SourceMismatch),
        SizeAmount::LowerBound { bytes: value } => VizQuantity::lower_bound("bytes", bytes(value)?),
        _ => VizQuantity::unknown("bytes", "viz.unavailable.no_attribution".into()),
    };
    value.ok_or(Error::InvalidQuantity)
}
