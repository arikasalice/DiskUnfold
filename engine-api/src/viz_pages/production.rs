//! Assembly of admitted single-parent projections into pinned page rows.
//! Legend classifications and display flags must come from their owning producer.
use super::{Error as PagesError, ModelInput, ModelPages};
use crate::display_path;
use du_analysis_index::{
    ResultVersion,
    folder_collection::{FolderCollection, Unenumerated, UnreadInside},
    visualization::{projection, ratio},
};
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultItem, ResultItemKind, ResultVersionRead},
    visualization::{
        item_kind::VizItemKind,
        quantity::VizQuantity,
        rows::{VizDenominator, VizDenominatorBasis, VizRatio, VizRow, VizUnread},
        type_slices::VizTypeSlice,
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

mod folder_table;
mod groups;
pub(crate) use folder_table::rows as folder_table_rows;
pub(crate) use groups::nested_group_rows;

/// Classification and flags already evaluated by their owning producer for one item.
#[derive(Clone)]
pub struct RowPresentation {
    pub item: ItemRef,
    pub legend_class_key: String,
    pub display_flags: Vec<String>,
}

/// Owner-provided presentation of the group that merges small items: the Other bar
/// of a ranking, or the merged small siblings of a treemap.
#[derive(Clone)]
pub struct OtherPresentation {
    /// Opaque reference scoped to this model. A treemap makes one group per parent
    /// and derives each reference itself, so it does not read this value.
    pub group_ref: String,
    /// Display name supplied by the presentation owner.
    pub display_name: String,
    /// Message-bundle key for the name of the group row, or none. When present the
    /// group row carries it as labelKey and the display name must be the same key
    /// string, as the fallback for a consumer that does not know labelKey.
    pub label_key: Option<String>,
    /// Classification selected by the presentation owner.
    pub legend_class_key: String,
    /// Display flags supplied by their owner.
    pub display_flags: Vec<String>,
    /// The group carries its members' type slices (F06 README 54.2): set by the
    /// presentation owner under the type criterion when nothing narrows the model.
    pub type_slices: bool,
}

/// Assembly refuses inputs when required source or presentation evidence is unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    SourceMismatch,
    MissingDisplayName,
    MissingClassification,
    InvalidQuantity,
    InvalidRatio,
    /// The projection merged small siblings but no group presentation was supplied,
    /// or the supplied one is unusable.
    MissingGroupPresentation,
    /// A group's members, total or position disagree with the projection's rows.
    InvalidGroup,
    Cancelled,
    Pages(PagesError),
}

/// Registers rows of one scope-denominator projection in the live model store.
///
/// The caller keeps `model` for geometry and supplies the same canonical fingerprint
/// used for the layout. Classifications and flags must come from the pinned model's
/// owning producers; this function does not classify or do I/O. When the projection
/// merged its small siblings into a group, other supplies that group's presentation;
/// the members then leave the table and are read through the group instead.
pub fn register_projection(
    pages: &ModelPages,
    version: Arc<ResultVersion>,
    model: &projection::Model,
    fingerprint: String,
    presentations: Vec<RowPresentation>,
    other: Option<&OtherPresentation>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<String, Error> {
    let input = project_rows(version, model, fingerprint, presentations, other, cancelled)?;
    pages.register(input).map_err(Error::Pages)
}

pub(super) fn project_rows(
    version: Arc<ResultVersion>,
    model: &projection::Model,
    fingerprint: String,
    presentations: Vec<RowPresentation>,
    other: Option<&OtherPresentation>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<ModelInput, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    validate_model(&version, model)?;
    // A filtered model shows only the children that keep a file; the others have no row.
    let presentations = if model.filter.is_some() {
        let shown: HashSet<u32> = model.rows.iter().map(|row| row.item.item_no).collect();
        presentations
            .into_iter()
            .filter(|presentation| shown.contains(&presentation.item.item_no))
            .collect()
    } else {
        presentations
    };
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
            cancelled,
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
    let (rows, groups) = groups::assemble(&version, model, rows, other, &denominator, cancelled)?;
    Ok(ModelInput {
        version,
        size_basis: model.basis.clone(),
        condition_fingerprint: fingerprint,
        rows,
        groups,
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
    let denominator = denominator(&total, basis)?;
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
        let slices = row_slices(&version, &presentation, basis, false, cancelled)?;
        rows.push(make_row(
            &version,
            item,
            &size,
            presentation,
            &denominator,
            slices,
        )?);
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(ModelInput {
        version,
        size_basis: basis.clone(),
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
    let original =
        original_size(version, model.parent.item_no, &model.basis).ok_or(Error::SourceMismatch)?;
    let children = du_analysis_index::browse::open(version, Some(&model.parent))
        .map_err(|_| Error::SourceMismatch)?
        .children
        .len();
    let rows_fit = if model.filter.is_some() {
        model.rows.len() <= children
    } else {
        model.rows.len() == children
    };
    if original != model.totals.scope || !rows_fit {
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
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<VizRow, Error> {
    let item = version
        .items()
        .get(row.item.item_no as usize)
        .ok_or(Error::SourceMismatch)?;
    if row.item.result_version != model.result_revision
        || item.parent != Some(model.parent.item_no)
        || !row_size_holds(
            original_size(version, item.item_no, &model.basis).as_ref(),
            &row.size,
            model.filter.is_some(),
        )
    {
        return Err(Error::SourceMismatch);
    }
    let filtered = model.filter.is_some();
    let slices = row_slices(version, &presentation, &model.basis, filtered, cancelled)?;
    let mut projected = make_row(version, item, &row.size, presentation, denominator, slices)?;
    let model_ratio = ratio::calculate(&quantity(&row.size)?, &denominator.value)
        .as_deref()
        .map(VizRatio::parse)
        .transpose()
        .map_err(|_| Error::InvalidRatio)?;
    let attributed_ratio = row
        .ratio
        .as_deref()
        .map(VizRatio::parse)
        .transpose()
        .map_err(|_| Error::InvalidRatio)?;
    let non_counted = shared_counted_item(version, item.item_no, &model.basis)?
        .is_some_and(|counted| counted.item_no != item.item_no);
    let allocated_folder =
        model.basis == SizeBasis::Allocated && item.kind == ResultItemKind::Directory;
    if !allocated_folder && attributed_ratio != model_ratio {
        return Err(Error::InvalidRatio);
    }
    let expected_ratio = if non_counted {
        None
    } else if allocated_folder {
        attributed_ratio
    } else {
        model_ratio
    };
    if allocated_folder {
        projected.ratio = expected_ratio.clone();
    }
    if projected.ratio != expected_ratio {
        return Err(Error::InvalidRatio);
    }
    Ok(projected)
}

/// A row shows its child's original value, or under a filter the kept part of it:
/// the same basis, no more bytes, and an unknown value for a child whose kept share
/// cannot be proven.
fn row_size_holds(original: Option<&SizeValue>, shown: &SizeValue, filtered: bool) -> bool {
    let Some(original) = original else {
        return false;
    };
    if !filtered {
        return original == shown;
    }
    let bytes = |value: &SizeValue| match &value.amount {
        du_core_contracts::SizeAmount::Exact { bytes }
        | du_core_contracts::SizeAmount::LowerBound { bytes } => Some(bytes.get()),
        du_core_contracts::SizeAmount::Unknown => None,
    };
    original.basis == shown.basis
        && match (bytes(original), bytes(shown)) {
            (Some(whole), Some(kept)) => kept <= whole,
            (_, None) => true,
            (None, Some(_)) => false,
        }
}

fn make_row(
    version: &ResultVersion,
    item: &ResultItem,
    size: &SizeValue,
    presentation: RowPresentation,
    denominator: &VizDenominator,
    type_slices: Option<Vec<VizTypeSlice>>,
) -> Result<VizRow, Error> {
    let (_, root) = version
        .observed_root_for(&presentation.item)
        .map_err(|_| Error::MissingDisplayName)?;
    let display_name =
        display_path::name(&root.path.encoding, &item.name).ok_or(Error::MissingDisplayName)?;
    let counted_item = shared_counted_item(version, item.item_no, &size.basis)?;
    let non_counted = counted_item
        .as_ref()
        .is_some_and(|counted| counted.item_no != item.item_no);
    let row_size = if non_counted {
        zero_non_counted_size(size.clone())
    } else {
        size.clone()
    };
    let ratio = if non_counted {
        None
    } else {
        let numerator = quantity(&row_size)?;
        ratio::calculate(&numerator, &denominator.value)
            .as_deref()
            .map(VizRatio::parse)
            .transpose()
            .map_err(|_| Error::InvalidRatio)?
    };
    let projected = VizRow {
        item_ref: Some(presentation.item),
        group_ref: None,
        display_name,
        size: row_size,
        quantity: None,
        ratio,
        denominator: denominator.clone(),
        legend_class_key: presentation.legend_class_key,
        display_flags: presentation.display_flags,
        label_key: None,
        member_count: None,
        item_kind: VizItemKind::from_result(item.kind),
        condition: None,
        shares_space: counted_item.as_ref().map(|_| true),
        shared_counted_item: if non_counted {
            counted_item.clone()
        } else {
            None
        },
        unread: row_unread(version, item),
        type_slices,
        links: None,
    };
    projected
        .validate_revision(version.version_id())
        .map_err(|_| Error::MissingClassification)?;
    Ok(projected)
}
/// Whether a folder row could not be read (F06 README 53): its own listing failed, or
/// ranges below it were not read. One function for every table, so the root table and
/// an expanded folder's table agree. Its own failure wins: nothing below it was counted.
fn row_unread(version: &ResultVersion, item: &ResultItem) -> Option<VizUnread> {
    let reference = ItemRef {
        result_version: version.version_id().clone(),
        item_no: item.item_no,
    };
    let collection = version.folder_collection(&reference).ok()?;
    match collection {
        FolderCollection::NotAFolder => None,
        FolderCollection::Folder {
            unenumerated: Unenumerated::Yes { .. },
            ..
        } => Some(VizUnread::Itself),
        FolderCollection::Folder { .. } => match version.unread_inside(&reference).ok()? {
            Some(UnreadInside::Recorded { count }) => VizUnread::inside(count),
            _ => None,
        },
    }
}

/// A row's type slices (F06 README 54, #298), made by the function that makes its
/// shape's.
fn row_slices(
    version: &ResultVersion,
    presentation: &RowPresentation,
    basis: &SizeBasis,
    filtered: bool,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<Vec<VizTypeSlice>>, Error> {
    super::type_slices::folder_slices(
        version,
        &presentation.item,
        &presentation.legend_class_key,
        basis,
        filtered,
        cancelled,
    )
    .map_err(|error| match error {
        super::type_slices::SlicesError::Cancelled => Error::Cancelled,
        super::type_slices::SlicesError::Invalid => Error::SourceMismatch,
    })
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
fn original_size(version: &ResultVersion, item_no: u32, basis: &SizeBasis) -> Option<SizeValue> {
    let mut size = version.item_space_size(item_no, basis.clone())?;
    if shared_counted_item(version, item_no, basis)
        .ok()?
        .is_some_and(|counted| counted.item_no != item_no)
    {
        size = zero_non_counted_size(size);
    }
    Some(size)
}

fn shared_counted_item(
    version: &ResultVersion,
    item_no: u32,
    basis: &SizeBasis,
) -> Result<Option<ItemRef>, Error> {
    if *basis != SizeBasis::Allocated
        || !version
            .items()
            .get(item_no as usize)
            .is_some_and(|item| item.kind == ResultItemKind::File)
    {
        return Ok(None);
    }
    version
        .shared_counted_item(&ItemRef {
            result_version: version.version_id().clone(),
            item_no,
        })
        .map_err(|_| Error::SourceMismatch)
}

fn zero_non_counted_size(mut size: SizeValue) -> SizeValue {
    size.amount = match size.amount {
        SizeAmount::Exact { .. } => SizeAmount::Exact { bytes: 0.into() },
        SizeAmount::LowerBound { .. } => SizeAmount::LowerBound { bytes: 0.into() },
        SizeAmount::Unknown => SizeAmount::Unknown,
    };
    size
}

fn quantity(size: &SizeValue) -> Result<VizQuantity, Error> {
    let bytes = |bytes: &du_core_contracts::ByteSize| Ok::<_, Error>(bytes.get());
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

/// Rows for the given children with the given values, such as the kept bytes of a
/// narrowed ranking (F06 README 47), against the parent's original total.
pub(super) fn project_sized(
    version: &ResultVersion,
    parent: &ItemRef,
    basis: &SizeBasis,
    sized: &[(ItemRef, SizeValue)],
    presentations: Vec<RowPresentation>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<VizRow>, Error> {
    let total = original_size(version, parent.item_no, basis).ok_or(Error::SourceMismatch)?;
    let denominator = denominator(&total, basis)?;
    let mut by_item = presentations_by_item(version.version_id(), presentations, cancelled)?;
    let mut rows = Vec::with_capacity(sized.len());
    for (item, size) in sized {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let observed = version
            .items()
            .get(item.item_no as usize)
            .filter(|observed| observed.parent == Some(parent.item_no))
            .ok_or(Error::SourceMismatch)?;
        let presentation = by_item
            .remove(&item.item_no)
            .ok_or(Error::MissingClassification)?;
        rows.push(make_row(
            version,
            observed,
            size,
            presentation,
            &denominator,
            None,
        )?);
    }
    Ok(rows)
}
