//! A narrow, homogeneous internal layout boundary for admitted F03 rows.
//!
//! This is not the public F06 model: cross-format quantities, totals, groups,
//! metadata and model references still require their published physical contract.
mod bar_strategy;
mod numeric_layout;
mod trend_strategy;
pub(super) use bar_strategy::BarLayout;
pub use bar_strategy::RankedBar;
pub use numeric_layout::{CommonInput, CommonRow};
pub use trend_strategy::{SelectedTrend, TrendDispatchError, TrendError, TrendInput, TrendOutput};
pub(super) use trend_strategy::{TrendLayout, trend_descriptor};

use super::{
    bar_categories, bar_projection,
    geometry::{NoArea, Rect, rectangle_geometry, valid_bounds},
    registry, root_ref, squarify, treemap,
};
use crate::ResultVersion;
use du_core_contracts::{
    ItemRef, SizeBasis, SizeValue,
    visualization::{LayoutDescriptor, LayoutStrategyKey, geometry::Geometry},
};

/// One admitted source row; its original F03 value is unchanged.
#[derive(Clone, Debug)]
pub struct NumericRow {
    item: ItemRef,
    path_compare_key: Option<Vec<u8>>,
    value: SizeValue,
    bytes: u64,
}
impl NumericRow {
    pub fn item(&self) -> &ItemRef {
        &self.item
    }
    /// The pinned comparison key, when available for this row.
    pub fn path_compare_key(&self) -> Option<&[u8]> {
        self.path_compare_key.as_deref()
    }
    pub fn value(&self) -> &SizeValue {
        &self.value
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

/// A disjoint sibling partition of attributed bytes from one pinned result.
/// Construction is restricted to the existing F03 observation checks.
pub struct Input {
    rows: Vec<NumericRow>,
    bounds: Rect,
}
impl Input {
    /// Validates attribution and keeps the original source values and keys.
    pub fn admit(
        version: &ResultVersion,
        parent: &ItemRef,
        basis: SizeBasis,
        bounds: Rect,
        keys: &[treemap::ChildKey],
    ) -> Result<Self, treemap::AdmissionError> {
        Self::admit_cancellable(version, parent, basis, bounds, keys, &mut || false)
    }
    /// Admits rows without layout, checking cancellation throughout traversal.
    pub fn admit_cancellable(
        version: &ResultVersion,
        parent: &ItemRef,
        basis: SizeBasis,
        bounds: Rect,
        keys: &[treemap::ChildKey],
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Self, treemap::AdmissionError> {
        if cancelled() {
            return Err(treemap::AdmissionError::Cancelled);
        }
        root_ref(version, parent).map_err(|_| treemap::AdmissionError::InvalidParent)?;
        if !valid_bounds(bounds) {
            return Err(treemap::AdmissionError::Layout(
                squarify::LayoutError::InvalidBounds,
            ));
        }
        let admitted = treemap::admit_cancellable(version, parent, basis, keys, cancelled)?;
        let mut rows = Vec::with_capacity(admitted.rows.len());
        for row in admitted.rows {
            if cancelled() {
                return Err(treemap::AdmissionError::Cancelled);
            }
            rows.push(NumericRow {
                item: row.child.item,
                path_compare_key: row.key,
                value: row.child.value,
                bytes: row.bytes,
            });
        }
        if cancelled() {
            return Err(treemap::AdmissionError::Cancelled);
        }
        Ok(Self { rows, bounds })
    }
    pub fn rows(&self) -> &[NumericRow] {
        &self.rows
    }
    pub fn bounds(&self) -> Rect {
        self.bounds
    }
}

/// A geometry linked to the same input row by index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    pub row_index: usize,
    pub geometry: Geometry,
}

/// Internal geometry result; source values remain in the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub shapes: Vec<Shape>,
    pub skipped: Vec<Omission>,
}

/// An input row with no drawable area and its preserved reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Omission {
    pub row_index: usize,
    pub reason: NoArea,
}

/// A calculation failure without a partial result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Cancelled,
    InvalidInput,
    ArithmeticOverflow,
    InvalidReference,
    Geometry(du_core_contracts::visualization::geometry::GeometryError),
}

/// Stateless typed strategy sharing one input and output shape.
pub trait Strategy: Send + Sync {
    fn layout(
        &self,
        _input: &Input,
        _cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Output, Error> {
        Err(Error::InvalidInput)
    }

    /// Computes geometry for common numeric rows and view conditions.
    fn layout_common(
        &self,
        input: &CommonInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Output, Error> {
        self.layout(input.legacy.ok_or(Error::InvalidInput)?, cancelled)
    }

    /// Computes geometry and its resolved axis in one calculation.
    fn layout_with_axis(
        &self,
        input: &CommonInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(Output, Option<u64>), Error> {
        self.layout_common(input, cancelled)
            .map(|output| (output, None))
    }

    /// Calculates a bar representation when the implementation supports it.
    fn bar(
        &self,
        _input: BarInput<'_>,
        _cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<bar_projection::Ranking, BarError> {
        Err(BarError::UnsupportedMode)
    }

    /// Calculates a classified representation from owner-prepared numeric rows.
    fn category_bar(
        &self,
        _input: bar_categories::Input<'_>,
        _cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<bar_categories::Layout, BarError> {
        Err(BarError::UnsupportedMode)
    }

    /// Positions admitted observations when the strategy supports time series.
    fn trend(
        &self,
        _input: TrendInput<'_>,
        _cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<TrendOutput, TrendError> {
        Err(TrendError::Unsupported)
    }

    /// Whether every published bar representation is implemented.
    fn complete_bar(&self) -> bool {
        false
    }
}

/// Distinct representations of the single published bar strategy.
pub enum BarInput<'a> {
    /// Immediate children ranked using a pinned F03 folder total.
    Ranked(bar_projection::ChildrenRequest<'a>),
    /// Validated pinned ranked facts prepared independently of strategy dispatch.
    PreparedRanked(Box<bar_projection::Prepared<'a>>),
    /// Classification evidence is not yet admitted by this typed boundary.
    TypeComparison,
    /// Legend-ordered segments and attribution evidence are not yet admitted.
    Stacked,
}

/// Bar-mode rejection or failure of the pinned F03 projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarError {
    UnsupportedMode,
    Projection(bar_projection::Error),
    Category(bar_categories::Error),
}

/// Identity of the single bar strategy, shared by its representations.
pub fn bar_descriptor() -> LayoutDescriptor {
    LayoutDescriptor {
        format: "bar".into(),
        strategy: LayoutStrategyKey {
            id: "viz.bar".into(),
            revision: 1,
        },
    }
}

/// Selected bar result retains its pinned F03 totals and source rows.
pub struct SelectedBar {
    pub descriptor: LayoutDescriptor,
    pub output: bar_projection::Ranking,
}

/// Selected category geometry and the descriptor that produced it.
pub struct SelectedCategoryBar {
    pub descriptor: LayoutDescriptor,
    pub output: bar_categories::Layout,
}

/// Selection failures remain distinct from unsupported modes and evidence errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarDispatchError {
    Selection(registry::SelectionError),
    Bar(BarError),
}

/// Treemap geometry for a single already admitted parent partition.
pub struct Treemap;
impl Strategy for Treemap {
    fn layout(&self, input: &Input, cancelled: &mut dyn FnMut() -> bool) -> Result<Output, Error> {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut weights = Vec::with_capacity(input.rows.len());
        for row in &input.rows {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            weights.push(treemap::layout_weight(&row.value, row.bytes));
        }
        let layout =
            squarify::layout_cancellable(input.bounds, &weights, cancelled).map_err(|error| {
                match error {
                    squarify::LayoutError::ArithmeticOverflow => Error::ArithmeticOverflow,
                    squarify::LayoutError::Cancelled => Error::Cancelled,
                    _ => Error::InvalidInput,
                }
            })?;
        let mut shapes = Vec::with_capacity(layout.shapes.len());
        for shape in layout.shapes {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let geometry = rectangle_geometry(shape.rect).map_err(Error::Geometry)?;
            shapes.push(Shape {
                row_index: shape.input_index,
                geometry,
            });
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut skipped = Vec::with_capacity(layout.skipped.len());
        for skip in layout.skipped {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let reason = match skip.reason {
                squarify::SkipReason::Unknown => NoArea::Unknown,
                squarify::SkipReason::ExactZero => NoArea::ExactZero,
                squarify::SkipReason::LowerBoundZero => NoArea::LowerBoundZero,
                squarify::SkipReason::CoordinatePrecision => NoArea::CoordinatePrecision,
            };
            skipped.push(Omission {
                row_index: skip.input_index,
                reason,
            });
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(Output { shapes, skipped })
    }
}

/// A selected strategy result and its exact registered identity.
pub struct Selected {
    pub descriptor: LayoutDescriptor,
    pub output: Output,
    /// Resolved axis maximum, when supplied by the selected layout.
    pub axis_max: Option<u64>,
}
/// A frozen catalog with homogeneous inputs and outputs.
pub struct Dispatcher(registry::Registry<Box<dyn Strategy>>);
impl Dispatcher {
    pub fn new(registry: registry::Registry<Box<dyn Strategy>>) -> Self {
        Self(registry)
    }
    pub fn catalog(&self) -> impl Iterator<Item = &LayoutDescriptor> {
        self.0.catalog()
    }
    /// Resolves a descriptor without invoking its layout implementation.
    ///
    /// Omitted keys select the highest registered revision for the format;
    /// explicit keys must match exactly. Format keys remain open strings.
    pub fn resolve(
        &self,
        format: &str,
        key: Option<&LayoutStrategyKey>,
    ) -> Result<&LayoutDescriptor, registry::SelectionError> {
        self.resolve_entry(format, key)
            .map(|entry| &entry.descriptor)
    }

    fn resolve_entry(
        &self,
        format: &str,
        key: Option<&LayoutStrategyKey>,
    ) -> Result<&registry::Registered<Box<dyn Strategy>>, registry::SelectionError> {
        match key {
            Some(key) => self.0.resolve(format, key),
            None => self.0.latest(format),
        }
    }

    /// Selects a registered strategy and executes one bar representation.
    pub fn execute_bar(
        &self,
        format: &str,
        key: Option<&LayoutStrategyKey>,
        input: BarInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<SelectedBar, BarDispatchError> {
        let entry = self
            .resolve_entry(format, key)
            .map_err(BarDispatchError::Selection)?;
        let output = entry
            .implementation
            .bar(input, cancelled)
            .map_err(BarDispatchError::Bar)?;
        if cancelled() {
            return Err(BarDispatchError::Bar(BarError::Projection(
                bar_projection::Error::Cancelled,
            )));
        }
        Ok(SelectedBar {
            descriptor: entry.descriptor.clone(),
            output,
        })
    }

    /// Executes a classification comparison or stack through the selected bar strategy.
    pub fn execute_category_bar(
        &self,
        key: Option<&LayoutStrategyKey>,
        input: bar_categories::Input<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<SelectedCategoryBar, BarDispatchError> {
        let entry = self
            .resolve_entry("bar", key)
            .map_err(BarDispatchError::Selection)?;
        let output = entry
            .implementation
            .category_bar(input, cancelled)
            .map_err(BarDispatchError::Bar)?;
        if cancelled() {
            return Err(BarDispatchError::Bar(BarError::Category(
                bar_categories::Error::Layout(super::bar::Error::Cancelled),
            )));
        }
        Ok(SelectedCategoryBar {
            descriptor: entry.descriptor.clone(),
            output,
        })
    }

    pub fn execute(
        &self,
        format: &str,
        key: Option<&LayoutStrategyKey>,
        input: &Input,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Selected, DispatchError> {
        let common = CommonInput::from_legacy(input);
        self.execute_common(format, key, &common, cancelled)
    }

    /// Selects a strategy and computes geometry from common numeric rows.
    pub fn execute_common(
        &self,
        format: &str,
        key: Option<&LayoutStrategyKey>,
        input: &CommonInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Selected, DispatchError> {
        let entry = self
            .resolve_entry(format, key)
            .map_err(DispatchError::Selection)?;
        let (output, axis_max) = entry
            .implementation
            .layout_with_axis(input, cancelled)
            .map_err(DispatchError::Layout)?;
        for shape in &output.shapes {
            if cancelled() {
                return Err(DispatchError::Layout(Error::Cancelled));
            }
            if shape.row_index >= input.row_count() {
                return Err(DispatchError::Layout(Error::InvalidReference));
            }
        }
        for skip in &output.skipped {
            if cancelled() {
                return Err(DispatchError::Layout(Error::Cancelled));
            }
            if skip.row_index >= input.row_count() {
                return Err(DispatchError::Layout(Error::InvalidReference));
            }
        }
        if cancelled() {
            return Err(DispatchError::Layout(Error::Cancelled));
        }
        Ok(Selected {
            descriptor: entry.descriptor.clone(),
            output,
            axis_max,
        })
    }
}

/// Registry selection and calculation failures remain distinct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchError {
    Selection(registry::SelectionError),
    Layout(Error),
}

/// The only currently admitted live format. This does not advertise full F06 readiness.
pub fn treemap_descriptor() -> LayoutDescriptor {
    LayoutDescriptor {
        format: "treemap".into(),
        strategy: LayoutStrategyKey {
            id: "viz.treemap".into(),
            revision: 1,
        },
    }
}

#[cfg(test)]
mod catalog_tests;
