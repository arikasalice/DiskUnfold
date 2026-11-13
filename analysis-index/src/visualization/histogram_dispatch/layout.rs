//! Histogram strategy over owner-prepared numeric rows.
use crate::visualization::{
    geometry::{self, Weight},
    histogram,
    typed_layout::{CommonInput, Error, Omission, Output, Shape, Strategy},
};
use du_core_contracts::visualization::{LayoutDescriptor, LayoutStrategyKey, quantity::VizAmount};
/// Histogram geometry over owner-prepared bin quantities.
pub struct HistogramLayout;
impl Strategy for HistogramLayout {
    fn layout_with_axis(
        &self,
        input: &CommonInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(Output, Option<u64>), Error> {
        if input.is_legacy() {
            return Err(Error::InvalidInput);
        }
        let mut weights = Vec::with_capacity(input.rows.len());
        let unit = input.rows.first().map(|row| row.quantity.unit());
        if unit.is_some_and(|unit| unit != "items" && unit != "bytes") {
            return Err(Error::InvalidInput);
        }
        for row in &input.rows {
            if Some(row.quantity.unit()) != unit {
                return Err(Error::InvalidInput);
            }
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let weight = match row.quantity.amount() {
                VizAmount::Exact { value } => {
                    Weight::Exact(u64::try_from(value.get()).map_err(|_| Error::InvalidInput)?)
                }
                VizAmount::LowerBound { value } => {
                    Weight::LowerBound(u64::try_from(value.get()).map_err(|_| Error::InvalidInput)?)
                }
                VizAmount::Unknown { .. } => Weight::Unknown,
            };
            weights.push(weight);
        }
        let (axis, rectangles) =
            histogram::layout_values(input.bounds, &weights, input.scale, cancelled).map_err(
                |error| {
                    if error == histogram::Error::Cancelled {
                        Error::Cancelled
                    } else {
                        Error::InvalidInput
                    }
                },
            )?;
        let mut shapes = Vec::new();
        let mut skipped = Vec::new();
        for (row_index, (rect, reason)) in rectangles.into_iter().enumerate() {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            if let Some(rect) = rect {
                shapes.push(Shape {
                    row_index,
                    geometry: geometry::rectangle_geometry(rect).map_err(Error::Geometry)?,
                });
            } else if let Some(reason) = reason {
                skipped.push(Omission { row_index, reason });
            } else {
                return Err(Error::InvalidInput);
            }
        }
        Ok((Output { shapes, skipped }, axis))
    }
}
/// Published identity of the histogram strategy.
pub fn histogram_descriptor() -> LayoutDescriptor {
    LayoutDescriptor {
        format: "histogram".into(),
        strategy: LayoutStrategyKey {
            id: "viz.histogram".into(),
            revision: 1,
        },
    }
}
