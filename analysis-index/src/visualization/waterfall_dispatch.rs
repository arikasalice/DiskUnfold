//! Waterfall geometry over owner-ordered comparison rows.

use super::{
    geometry::{Weight, rectangle_geometry},
    typed_layout::{CommonInput, Error, Omission, Output, Shape, Strategy},
    waterfall::{self, Change, Direction},
};
use du_core_contracts::visualization::{
    LayoutDescriptor, LayoutStrategyKey,
    quantity::{VizAmount, VizQuantity},
};

pub(super) struct WaterfallLayout;

impl Strategy for WaterfallLayout {
    /// Admits a start total, signed comparison rows, and an independent end total.
    fn layout_with_axis(
        &self,
        input: &CommonInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(Output, Option<u64>), Error> {
        let [start, deltas @ .., end] = input.rows.as_slice() else {
            return Err(Error::InvalidInput);
        };
        let (_, start) = quantity(&start.quantity, false)?;
        let (_, end) = quantity(&end.quantity, false)?;
        let mut changes = Vec::with_capacity(deltas.len());
        for (index, row) in deltas.iter().enumerate() {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let (direction, value) = quantity(&row.quantity, true)?;
            changes.push(Change {
                item_no: u32::try_from(index).map_err(|_| Error::ArithmeticOverflow)?,
                direction,
                value,
            });
        }
        let layout = waterfall::layout_cancellable(
            input.bounds,
            start,
            &changes,
            end,
            input.scale,
            cancelled,
        )
        .map_err(|error| match error {
            waterfall::Error::Cancelled => Error::Cancelled,
            _ => Error::InvalidInput,
        })?;
        let bars = std::iter::once(layout.start)
            .chain(layout.changes)
            .chain(std::iter::once(layout.end));
        let mut shapes = Vec::new();
        let mut skipped = Vec::new();
        for (row_index, bar) in bars.enumerate() {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            if let Some(rect) = bar.rect {
                shapes.push(Shape {
                    row_index,
                    geometry: rectangle_geometry(rect).map_err(Error::Geometry)?,
                });
            } else if let Some(reason) = bar.no_area {
                skipped.push(Omission { row_index, reason });
            } else {
                return Err(Error::InvalidInput);
            }
        }
        Ok((Output { shapes, skipped }, layout.axis_max))
    }
}

fn quantity(value: &VizQuantity, signed: bool) -> Result<(Direction, Weight), Error> {
    if value.unit() != "bytes" {
        return Err(Error::InvalidInput);
    }
    let result = match value.amount() {
        VizAmount::Exact { value } => {
            let value = value.get();
            if value < 0 && !signed {
                return Err(Error::InvalidInput);
            }
            let bytes = u64::try_from(value.unsigned_abs()).map_err(|_| Error::InvalidInput)?;
            let direction = if value < 0 {
                Direction::Decrease
            } else {
                Direction::Increase
            };
            (direction, Weight::Exact(bytes))
        }
        VizAmount::LowerBound { value } => (
            Direction::Increase,
            Weight::LowerBound(u64::try_from(value.get()).map_err(|_| Error::InvalidInput)?),
        ),
        // The layout never uses an unknown value's direction to establish an offset.
        VizAmount::Unknown { .. } => (Direction::Increase, Weight::Unknown),
    };
    Ok(result)
}

pub(super) fn descriptor() -> LayoutDescriptor {
    LayoutDescriptor {
        format: "waterfall".into(),
        strategy: LayoutStrategyKey {
            id: "viz.waterfall".into(),
            revision: 1,
        },
    }
}
