//! GUI use cases for an existing visualization session.
use std::sync::Arc;

use du_analysis_index::visualization::projection::Location;
use du_core_contracts::visualization::{
    locate::{VizLocate, VizLocateRequest},
    rows::{VizGroupRows, VizGroupRowsRequest, VizRows, VizRowsRequest, VizSizeRange},
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

/// Registers group-member reads on the same session that owns the rendered models.
pub fn register_group_rows(builder: &mut EngineBuilder, session: Arc<Session>) {
    builder.register(GroupRows(session));
}

/// Registers item location for the same session that owns the rendered models.
pub fn register_locate(builder: &mut EngineBuilder, session: Arc<Session>) {
    builder.register(Locate(session));
}

struct Rows(Arc<Session>);
struct GroupRows(Arc<Session>);
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

impl UseCase for GroupRows {
    const NAME: &'static str = "viz.group_rows";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizGroupRowsRequest;
    type Response = VizGroupRows;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        if request.validate().is_err() {
            return Err(EngineError::reason("command.body_invalid"));
        }
        let page = self
            .0
            .group_rows(
                &request.model_ref,
                &request.group_ref,
                request.start,
                request.count,
            )
            .map_err(rows_error)?;
        let response = VizGroupRows {
            rows: page.rows,
            total_count: page.total_count,
            group_total: page.group_total,
            size_range: match page.size_range {
                du_analysis_index::visualization::live_models::SizeRange::Known { min, max } => {
                    VizSizeRange::Known { min, max }
                }
                du_analysis_index::visualization::live_models::SizeRange::Unknown => {
                    VizSizeRange::Unknown
                }
            },
            result_revision: page.result_revision,
            observed_at: page.observed_at,
            completeness: page.completeness,
        };
        response
            .validate()
            .map_err(|_| EngineError::reason("support.internal_error"))?;
        Ok(response)
    }
}

fn rows_error(error: Error) -> EngineError {
    EngineError::reason(match error {
        Error::InvalidRange | Error::InvalidGroup => "command.body_invalid",
        Error::ModelExpired => "viz.model_expired",
        Error::StaleVersion => "result.version_stale",
        Error::ItemUnavailable => "ref.unavailable",
        _ => "support.internal_error",
    })
}
