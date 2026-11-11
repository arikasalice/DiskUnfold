//! Registered trend strategy for fixed, pre-decimated observations.

use super::{Dispatcher, Strategy};
use crate::visualization::{geometry::Rect, registry, trend};
use du_core_contracts::visualization::{
    LayoutDescriptor, LayoutStrategyKey,
    quantity::{VizAmount, VizQuantity},
};

/// Owner-prepared series with actual observation timestamps and original quantities.
pub struct TrendInput<'a> {
    pub bounds: Rect,
    pub series: &'a [trend::Series<VizQuantity>],
}

/// Trend geometry retains signed byte deltas and the original observation evidence.
pub type TrendOutput = trend::Trend<VizQuantity, i128>;

/// Trend-specific admission and layout failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrendError {
    Unsupported,
    InvalidUnit,
    InsufficientObservations,
    Layout(trend::Error),
}

/// Exact strategy selection and trend calculation report separate failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrendDispatchError {
    Selection(registry::SelectionError),
    Trend(TrendError),
}

/// The selected strategy and the positioned observations that it produced.
pub struct SelectedTrend {
    pub descriptor: LayoutDescriptor,
    pub output: TrendOutput,
}

impl Dispatcher {
    /// Runs a registered trend strategy over one admitted source.
    pub fn execute_trend(
        &self,
        key: Option<&LayoutStrategyKey>,
        input: TrendInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<SelectedTrend, TrendDispatchError> {
        let entry = self
            .resolve_entry("trend", key)
            .map_err(TrendDispatchError::Selection)?;
        let output = entry
            .implementation
            .trend(input, cancelled)
            .map_err(TrendDispatchError::Trend)?;
        if cancelled() {
            return Err(TrendDispatchError::Trend(TrendError::Layout(
                trend::Error::Cancelled,
            )));
        }
        Ok(SelectedTrend {
            descriptor: entry.descriptor.clone(),
            output,
        })
    }
}

pub(in crate::visualization) struct TrendLayout;

impl Strategy for TrendLayout {
    fn trend(
        &self,
        input: TrendInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<TrendOutput, TrendError> {
        validate(input.series, cancelled)?;
        trend::layout_values(input.bounds, input.series, numeric, cancelled)
            .map_err(TrendError::Layout)
    }
}

fn validate(
    series: &[trend::Series<VizQuantity>],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), TrendError> {
    let mut first = None;
    let mut distinct = false;
    for series in series {
        for point in &series.points {
            if cancelled() {
                return Err(TrendError::Layout(trend::Error::Cancelled));
            }
            if point.value.unit() != "bytes" {
                return Err(TrendError::InvalidUnit);
            }
            if numeric(&point.value).is_some() {
                distinct |= *first.get_or_insert(point.observed_at_ms) != point.observed_at_ms;
            }
        }
    }
    if cancelled() {
        return Err(TrendError::Layout(trend::Error::Cancelled));
    }
    distinct
        .then_some(())
        .ok_or(TrendError::InsufficientObservations)
}

fn numeric(quantity: &VizQuantity) -> Option<i128> {
    match quantity.amount() {
        VizAmount::Exact { value } | VizAmount::LowerBound { value } => Some(value.get()),
        VizAmount::Unknown { .. } => None,
    }
}

pub(in crate::visualization) fn trend_descriptor() -> LayoutDescriptor {
    LayoutDescriptor {
        format: "trend".into(),
        strategy: LayoutStrategyKey {
            id: "viz.trend".into(),
            revision: 1,
        },
    }
}
