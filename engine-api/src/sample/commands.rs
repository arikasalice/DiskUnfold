//! The four GUI-only sample use cases.

use std::sync::Arc;

use du_core_contracts::sample_ipc::{
    SampleClosed, SampleOpenRequest, SampleSession, SampleSessionRequest,
};

use super::SampleSessions;
use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

pub(super) fn register(builder: &mut EngineBuilder, sessions: &Arc<SampleSessions>) {
    builder.register(Open(sessions.clone()));
    builder.register(Status(sessions.clone()));
    builder.register(Reset(sessions.clone()));
    builder.register(Close(sessions.clone()));
}

struct Open(Arc<SampleSessions>);
struct Status(Arc<SampleSessions>);
struct Reset(Arc<SampleSessions>);
struct Close(Arc<SampleSessions>);

impl UseCase for Open {
    const NAME: &'static str = "sample.open";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SampleOpenRequest;
    type Response = SampleSession;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, caller: &CallerCtx) -> Result<Self::Response, EngineError> {
        self.0.open(&caller.correlation_id)
    }
}

impl UseCase for Status {
    const NAME: &'static str = "sample.status";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SampleSessionRequest;
    type Response = SampleSession;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0.status(&request.session_ref)
    }
}

impl UseCase for Reset {
    const NAME: &'static str = "sample.reset";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SampleSessionRequest;
    type Response = SampleSession;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0.reset(&request.session_ref, &caller.correlation_id)
    }
}

impl UseCase for Close {
    const NAME: &'static str = "sample.close";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SampleSessionRequest;
    type Response = SampleClosed;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0.close(&request.session_ref)
    }
}
