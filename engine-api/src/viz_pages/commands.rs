//! GUI use cases for an existing visualization session.
use std::sync::Arc;

use du_analysis_index::visualization::projection::Location;
use du_core_contracts::visualization::{
    locate::{VizLocate, VizLocateRequest},
    rows::{VizRows, VizRowsRequest},
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

use super::{Error, session::Session};

/// Registers row reads using the same session that owns the rendered models.
pub fn register_rows(builder: &mut EngineBuilder, session: Arc<Session>) {
    builder.register(Rows(session));
}

/// Registers item location for the same session that owns the rendered models.
pub fn register_locate(builder: &mut EngineBuilder, session: Arc<Session>) {
    builder.register(Locate(session));
}

struct Rows(Arc<Session>);
struct Locate(Arc<Session>);

impl UseCase for Rows {
    const NAME: &'static str = "viz.rows";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizRowsRequest;
    type Response = VizRows;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0.rows(&request).map_err(rows_error)
    }
}

impl UseCase for Locate {
    const NAME: &'static str = "viz.locate";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizLocateRequest;
    type Response = VizLocate;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        match self
            .0
            .locate(&request.model_ref, &request.item_ref)
            .map_err(rows_error)?
        {
            Location::Shape(index) => Ok(VizLocate::Shape {
                shape_index: u32::try_from(index)
                    .map_err(|_| EngineError::reason("support.internal_error"))?,
            }),
            Location::OutsideView => Ok(VizLocate::OutsideView {}),
            Location::OutsideScope => Ok(VizLocate::OutsideScope {}),
        }
    }
}

/// Maps page and location failures to the F06 GUI query contract.
fn rows_error(error: Error) -> EngineError {
    EngineError::reason(match error {
        Error::InvalidRange => "command.body_invalid",
        Error::ModelExpired => "viz.model_expired",
        Error::StaleVersion => "result.requery_required",
        Error::ItemUnavailable => "ref.unavailable",
        _ => "support.internal_error",
    })
}
