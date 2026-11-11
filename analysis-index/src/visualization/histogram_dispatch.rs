//! Owner-side preparation of pinned histogram evidence for common dispatch.
mod layout;
use crate::{
    ResultVersion,
    visualization::{
        geometry::{Rect, Scale, Weight},
        histogram, histogram_projection,
        typed_layout::{CommonInput, CommonRow, DispatchError, Dispatcher, Selected},
        unit_bins,
    },
};
use du_core_contracts::{
    ItemRef, SizeBasis, size_units::UnitTable, visualization::quantity::VizQuantity,
};
pub use layout::{HistogramLayout, histogram_descriptor};

/// Explicitly selected source, unit table and view policy; no defaults are chosen.
pub struct Request<'a> {
    pub version: &'a ResultVersion,
    pub scope: &'a ItemRef,
    pub size_basis: SizeBasis,
    pub unit_table: &'a UnitTable,
    pub unit_table_revision: u32,
    pub bounds: Rect,
    pub basis: histogram::Basis,
    pub scale: Scale,
    /// Owner-supplied reason used for a bin whose quantity is unknown.
    pub unknown_reason: &'a str,
}

/// Retains the same source, bin membership and unit table as the common rows.
pub struct Prepared {
    unit_bins: unit_bins::UnitBins,
    size_basis: SizeBasis,
    model: histogram_projection::Model,
    input: CommonInput<'static>,
}

/// One calculation and the owner evidence referenced by its row indices.
pub struct Rendered {
    prepared: Prepared,
    selected: Selected,
}

impl Prepared {
    /// Returns the interval boundaries and unit-table revision used for these rows.
    pub fn unit_bins(&self) -> &unit_bins::UnitBins {
        &self.unit_bins
    }

    /// Returns the size basis used to assign observations to intervals.
    pub fn size_basis(&self) -> &SizeBasis {
        &self.size_basis
    }

    /// Returns the pinned aggregate and interval membership in input row order.
    pub fn model(&self) -> &histogram_projection::Model {
        &self.model
    }

    /// Borrows the numeric input without allowing it to diverge from the evidence.
    pub fn input(&self) -> &CommonInput<'static> {
        &self.input
    }

    /// Executes this exact prepared input and retains its rows with the output.
    pub fn render(
        self,
        dispatcher: &Dispatcher,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Rendered, DispatchError> {
        let descriptor = histogram_descriptor();
        let selected = dispatcher.execute_common(
            &descriptor.format,
            Some(&descriptor.strategy),
            &self.input,
            cancelled,
        )?;
        Ok(Rendered {
            prepared: self,
            selected,
        })
    }
}

impl Rendered {
    /// Returns the exact input and evidence addressed by the geometry row indices.
    pub fn prepared(&self) -> &Prepared {
        &self.prepared
    }

    /// Returns the selected strategy, its geometry and its resolved axis.
    pub fn selected(&self) -> &Selected {
        &self.selected
    }
}

/// Preparation fails without returning partial rows or evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnitTable(unit_bins::Error),
    Projection(histogram_projection::Error),
    InvalidQuantity,
    Cancelled,
}

/// Builds common numeric rows and preserves their pinned owner evidence.
pub fn prepare(
    request: Request<'_>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Prepared, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let unit_bins = unit_bins::from_unit_table(request.unit_table, request.unit_table_revision)
        .map_err(Error::UnitTable)?;
    let model = histogram_projection::prepare(
        histogram_projection::Request {
            version: request.version,
            scope: request.scope,
            size_basis: request.size_basis.clone(),
            bins: &unit_bins.bins,
            bounds: request.bounds,
            histogram_basis: request.basis,
            scale: request.scale,
        },
        cancelled,
    )
    .map_err(|e| {
        if e == histogram_projection::Error::Cancelled {
            Error::Cancelled
        } else {
            Error::Projection(e)
        }
    })?;
    let mut rows = Vec::with_capacity(model.histogram.bins.len());
    for bin in &model.histogram.bins {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let (unit, value) = match request.basis {
            histogram::Basis::ItemCount => ("items", bin.item_count),
            histogram::Basis::Bytes => ("bytes", bin.bytes),
        };
        let quantity = match value {
            Weight::Exact(n) => VizQuantity::exact(unit, i128::from(n)),
            Weight::LowerBound(n) => VizQuantity::lower_bound(unit, n),
            Weight::Unknown => VizQuantity::unknown(unit, request.unknown_reason.to_owned()),
        }
        .ok_or(Error::InvalidQuantity)?;
        rows.push(CommonRow { quantity });
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(Prepared {
        unit_bins,
        size_basis: request.size_basis,
        model,
        input: CommonInput::new(rows, request.bounds, request.scale),
    })
}
