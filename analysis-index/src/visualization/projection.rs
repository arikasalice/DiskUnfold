//! Single-parent, pinned visualization projection for the admitted layout boundary.
//!
//! This is an internal projection, not the public F06 response: callers must
//! supply policy, provenance, model identity, paging and IPC contracts separately.
use super::{
    geometry::{NoArea, Rect},
    model_links::{ModelLinkError, validate_model_links},
    ratio,
    treemap::ChildKey,
    typed_layout::{self, Dispatcher, Input},
};
use crate::ResultVersion;
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeValue, Timestamp,
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead},
    visualization::{
        LayoutDescriptor, LayoutStrategyKey,
        geometry::Geometry,
        model::{ShapeMeta, VizRelationship},
    },
};
use std::num::NonZeroU64;

/// An original F03 row shared by the drawn shape and fallback list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub item: ItemRef,
    pub size: SizeValue,
    /// First child folder of the navigation root, or no established branch.
    pub root_branch_ref: Option<ItemRef>,
    /// Source value state, independent of whether the row has geometry.
    pub area_value: super::AreaValue,
    /// Layout omission reason, or `None` when the row has a shape.
    pub omission: Option<NoArea>,
    /// Immediate children outside this single-level projection, not a recursive count.
    pub omitted_children: usize,
    /// Ratio to the admitted parent's exact value in the selected size basis.
    pub ratio: Option<String>,
}
/// A format-independent shape referring to an entry in `rows`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    pub row_index: usize,
    pub meta: ShapeMeta,
    pub geometry: Geometry,
}
/// Exact byte totals for this unfiltered, ungrouped single-parent projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Totals {
    /// Original F03 set aggregate, never derived from geometry.
    pub scope: SizeValue,
    pub drawn_bytes: u64,
    pub grouped_bytes: u64,
    pub filtered_bytes: u64,
}
/// Immutable projection; it does not assign a public model reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub result_revision: Id,
    pub parent: ItemRef,
    pub basis: SizeBasis,
    pub completeness: ResultCompleteness,
    pub observed_at: Option<Timestamp>,
    pub strategy: LayoutDescriptor,
    pub rows: Vec<Row>,
    pub shapes: Vec<Shape>,
    pub relationships: Vec<VizRelationship>,
    pub omissions: Vec<typed_layout::Omission>,
    pub totals: Totals,
}

/// An observed item's location relative to this ungrouped projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    /// Zero-based position in the model's geometry array.
    Shape(usize),
    /// Inside the pinned folder scope, with no geometry in this view.
    OutsideView,
    /// Observed in the same result, outside the pinned folder scope.
    OutsideScope,
}

impl Model {
    /// Locates an exact item reference using only the pinned hierarchy and model.
    ///
    /// # Errors
    /// Returns `StaleVersion` for another revision and `NotFound` for an unknown item.
    pub fn locate(
        &self,
        version: &ResultVersion,
        item: &ItemRef,
    ) -> Result<Location, crate::QueryError> {
        if version.version_id() != &self.result_revision {
            return Err(crate::QueryError::StaleVersion);
        }
        let item_no = version.detail(item)?.item.item_no;
        if let Some(index) = self.shapes.iter().position(|shape| {
            self.rows
                .get(shape.row_index)
                .is_some_and(|row| row.item == *item)
        }) {
            return Ok(Location::Shape(index));
        }
        Ok(
            if super::descendant(version.items(), item_no, self.parent.item_no) {
                Location::OutsideView
            } else {
                Location::OutsideScope
            },
        )
    }
}
/// No model is produced on failure or cancellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidSource,
    UnavailableAttribution,
    OrderingUnavailable,
    Dispatch(typed_layout::DispatchError),
    InvalidLayout,
    ShapeLimitExceeded,
    Links(ModelLinkError),
    Cancelled,
}

/// Selection and view limit supplied by the caller.
pub struct Options<'a> {
    pub format: &'a str,
    pub strategy: Option<&'a LayoutStrategyKey>,
    pub max_shapes: usize,
}

/// One pinned source and the available immediate-child ordering keys.
pub struct Source<'a> {
    pub version: &'a ResultVersion,
    pub parent: &'a ItemRef,
    pub basis: SizeBasis,
    pub bounds: Rect,
    pub keys: &'a [ChildKey],
}

/// Admits and projects rows once from the same immutable version and parent.
///
/// Scope is exactly the parent, with no filtering or grouping. The limit is a
/// caller-supplied upper bound, not the complete published level-of-detail policy.
/// Consumers must not treat this projection as a complete public render response.
pub fn project(
    source: Source<'_>,
    dispatcher: &Dispatcher,
    options: Options<'_>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Model, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let Source {
        version,
        parent,
        basis,
        bounds,
        keys,
    } = source;
    let input = Input::admit_cancellable(version, parent, basis.clone(), bounds, keys, cancelled)
        .map_err(|e| match e {
        super::treemap::AdmissionError::Cancelled => Error::Cancelled,
        super::treemap::AdmissionError::NoAttribution => Error::UnavailableAttribution,
        super::treemap::AdmissionError::OrderingUnavailable => Error::OrderingUnavailable,
        _ => Error::InvalidSource,
    })?;
    let parent_value =
        super::value(version, parent.item_no, &basis).map_err(|_| Error::InvalidSource)?;
    let selected = dispatcher
        .execute(options.format, options.strategy, &input, cancelled)
        .map_err(Error::Dispatch)?;
    if selected.output.shapes.len() > options.max_shapes {
        return Err(Error::ShapeLimitExceeded);
    }
    let mut visited = vec![false; input.rows().len()];
    let mut omission_reasons = vec![None; input.rows().len()];
    let mut drawn_bytes = 0_u64;
    let mut shapes = Vec::with_capacity(selected.output.shapes.len());
    for shape in selected.output.shapes {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let row = input
            .rows()
            .get(shape.row_index)
            .ok_or(Error::InvalidLayout)?;
        if row.bytes() == 0 || std::mem::replace(&mut visited[shape.row_index], true) {
            return Err(Error::InvalidLayout);
        }
        drawn_bytes = drawn_bytes
            .checked_add(row.bytes())
            .ok_or(Error::InvalidLayout)?;
        let kind = &version.items()[row.item().item_no as usize].kind;
        let visible_depth = 1;
        shapes.push(Shape {
            row_index: shape.row_index,
            meta: ShapeMeta {
                shape_ref: format!("s{:x}-{visible_depth}", row.item().item_no),
                parent_shape_ref: None,
                visible_depth,
                root_branch_ref: (*kind == ResultItemKind::Directory).then(|| row.item().clone()),
            },
            geometry: shape.geometry,
        });
    }
    for omitted in &selected.output.skipped {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let row = input
            .rows()
            .get(omitted.row_index)
            .ok_or(Error::InvalidLayout)?;
        if std::mem::replace(&mut visited[omitted.row_index], true)
            || !matches!(
                (omitted.reason, row.bytes()),
                (NoArea::ExactZero | NoArea::LowerBoundZero, 0)
                    | (NoArea::CoordinatePrecision, 1..)
            )
        {
            return Err(Error::InvalidLayout);
        }
        omission_reasons[omitted.row_index] = Some(omitted.reason);
    }
    if visited.iter().any(|v| !v) {
        return Err(Error::InvalidLayout);
    }
    let relationships = Vec::new();
    validate_model_links(
        version.version_id(),
        &shapes.iter().map(|s| s.meta.clone()).collect::<Vec<_>>(),
        &relationships,
    )
    .map_err(Error::Links)?;
    let rows = project_rows(version, &input, &parent_value, &omission_reasons, cancelled)?;
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(Model {
        result_revision: version.version_id().clone(),
        parent: parent.clone(),
        basis,
        completeness: version.completeness(),
        observed_at: version.observed_at().cloned(),
        strategy: selected.descriptor,
        rows,
        shapes,
        relationships,
        omissions: selected.output.skipped,
        totals: Totals {
            scope: parent_value,
            drawn_bytes,
            grouped_bytes: 0,
            filtered_bytes: 0,
        },
    })
}

fn project_rows(
    version: &ResultVersion,
    input: &Input,
    parent_value: &SizeValue,
    omission_reasons: &[Option<NoArea>],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Vec<Row>, Error> {
    let denominator = match &parent_value.amount {
        SizeAmount::Exact { bytes } => {
            NonZeroU64::new(bytes.as_str().parse().map_err(|_| Error::InvalidSource)?)
        }
        SizeAmount::LowerBound { .. } => None,
        SizeAmount::Unknown => return Err(Error::UnavailableAttribution),
    };
    let mut rows = Vec::with_capacity(input.rows().len());
    for (index, row) in input.rows().iter().enumerate() {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        rows.push(Row {
            item: row.item().clone(),
            size: row.value().clone(),
            root_branch_ref: (version.items()[row.item().item_no as usize].kind
                == ResultItemKind::Directory)
                .then(|| row.item().clone()),
            area_value: super::area(row.value()),
            omission: omission_reasons[index],
            omitted_children: version.children(row.item().item_no).len(),
            ratio: denominator.map(|total| ratio::format_ratio(i128::from(row.bytes()), total)),
        });
    }
    Ok(rows)
}
