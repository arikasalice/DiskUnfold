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
    let mut by_item = HashMap::new();
    for p in presentations {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if p.item.result_version != model.result_revision
            || p.legend_class_key.is_empty()
            || by_item.insert(p.item.item_no, p).is_some()
        {
            return Err(Error::MissingClassification);
        }
    }
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
    let scope = version
        .space(model.parent.item_no)
        .ok_or(Error::SourceMismatch)?;
    let original = match model.basis {
        SizeBasis::Logical => &scope.logical,
        SizeBasis::Allocated => &scope.allocated,
        SizeBasis::UniquePhysical => &scope.unique_physical,
    };
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
        || version.space(item.item_no).map(|space| match model.basis {
            SizeBasis::Logical => &space.logical,
            SizeBasis::Allocated => &space.allocated,
            SizeBasis::UniquePhysical => &space.unique_physical,
        }) != Some(&row.size)
    {
        return Err(Error::SourceMismatch);
    }
    let (_, root) = version
        .observed_root_for(&row.item)
        .map_err(|_| Error::MissingDisplayName)?;
    let display_name =
        browse::display(&root.path.encoding, &item.name).ok_or(Error::MissingDisplayName)?;
    let numerator = exact_quantity(&row.size, &model.basis)?;
    if row.ratio != ratio::calculate(&numerator, &denominator.value) {
        return Err(Error::InvalidRatio);
    }
    let ratio = row
        .ratio
        .as_deref()
        .map(VizRatio::parse)
        .transpose()
        .map_err(|_| Error::InvalidRatio)?;
    let projected = VizRow {
        item_ref: Some(row.item.clone()),
        group_ref: None,
        display_name,
        size: row.size.clone(),
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
    Ok(VizDenominator {
        basis: VizDenominatorBasis::Scope,
        value: exact_quantity(size, basis)?,
        size: Some(size.clone()),
    })
}
fn exact_quantity(size: &SizeValue, basis: &SizeBasis) -> Result<VizQuantity, Error> {
    if &size.basis != basis || size.evidence == SizeEvidence::Unknown {
        return Err(Error::SourceMismatch);
    }
    let SizeAmount::Exact { bytes } = &size.amount else {
        return Err(Error::InvalidQuantity);
    };
    let value = bytes
        .as_str()
        .parse::<u64>()
        .map_err(|_| Error::InvalidQuantity)?;
    VizQuantity::exact("bytes", i128::from(value)).ok_or(Error::InvalidQuantity)
}
