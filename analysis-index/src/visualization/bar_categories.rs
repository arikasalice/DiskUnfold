//! Category bars over numeric facts from the pinned classification owner.

use super::{
    bar,
    geometry::{Rect, Scale, Weight},
};
use du_core_contracts::{
    result::ResultCompleteness,
    visualization::quantity::{VizAmount, VizQuantity},
};

/// One attributed byte amount identified by its registered legend key.
pub struct Row {
    pub legend_key: u32,
    pub quantity: VizQuantity,
}

/// Comparison and stack evidence use the same registered bar strategy.
pub enum Input<'a> {
    Comparison {
        bounds: Rect,
        rows: &'a [Row],
        scale: Scale,
        completeness: ResultCompleteness,
    },
    Stacked {
        bounds: Rect,
        rows: &'a [Row],
        axis_max: u64,
        legend: &'a [u32],
    },
}

/// Calculated geometry. The caller retains the original quantities and their evidence.
#[derive(Debug, PartialEq, Eq)]
pub enum Layout {
    Comparison(bar::Ranking),
    Stacked(bar::Stack),
}

/// Unusable category data or failure to calculate geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidQuantity,
    Layout(bar::Error),
}

pub(super) fn calculate(
    input: Input<'_>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Layout, Error> {
    match input {
        Input::Comparison {
            bounds,
            rows,
            scale,
            completeness,
        } => {
            let items = convert(rows, cancelled, |key, value| bar::Item {
                item_no: key,
                value,
            })?;
            bar::ranking_cancellable(bounds, &items, items.len(), scale, completeness, cancelled)
                .map(Layout::Comparison)
                .map_err(Error::Layout)
        }
        Input::Stacked {
            bounds,
            rows,
            axis_max,
            legend,
        } => {
            let segments = convert(rows, cancelled, |legend_key, value| bar::Segment {
                legend_key,
                value,
            })?;
            bar::stacked_cancellable(bounds, axis_max, legend, &segments, cancelled)
                .map(Layout::Stacked)
                .map_err(Error::Layout)
        }
    }
}

fn convert<T>(
    rows: &[Row],
    cancelled: &mut dyn FnMut() -> bool,
    mapped: impl Fn(u32, Weight) -> T,
) -> Result<Vec<T>, Error> {
    rows.iter()
        .map(|row| {
            if cancelled() {
                return Err(Error::Layout(bar::Error::Cancelled));
            }
            let value = &row.quantity;
            if value.unit() != "bytes" {
                return Err(Error::InvalidQuantity);
            }
            let weight = match value.amount() {
                VizAmount::Exact { value } => {
                    Weight::Exact(u64::try_from(value.get()).map_err(|_| Error::InvalidQuantity)?)
                }
                VizAmount::LowerBound { value } => Weight::LowerBound(
                    u64::try_from(value.get()).map_err(|_| Error::InvalidQuantity)?,
                ),
                VizAmount::Unknown { .. } => Weight::Unknown,
            };
            Ok(mapped(row.legend_key, weight))
        })
        .collect()
}
