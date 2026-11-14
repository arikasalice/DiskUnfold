use super::*;
use crate::api::{CallerCtx, Channel, UseCase};
use du_core_contracts::visualization::{
    cancel::{VizCancelRequest, VizCancelResponse},
    request::{VizViewChannel, VizViewOpenRequest},
};
pub(super) fn register(builder: &mut EngineBuilder, runtime: Arc<Runtime>) {
    builder.register(Open(runtime.clone()));
    builder.register(Close(runtime.clone()));
    builder.register(Render(runtime.clone()));
    builder.register(Cancel(runtime));
}
struct Open(Arc<Runtime>);
struct Close(Arc<Runtime>);
struct Render(Arc<Runtime>);
struct Cancel(Arc<Runtime>);
impl UseCase for Open {
    const NAME: &'static str = "viz.view.open";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizViewOpenRequest;
    type Response = VizViewChannel;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        Ok(VizViewChannel {
            view_channel: self.0.open()?,
        })
    }
}
impl UseCase for Close {
    const NAME: &'static str = "viz.view.close";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizViewChannel;
    type Response = VizViewOpenRequest;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0.close(&request.view_channel)?;
        Ok(VizViewOpenRequest {})
    }
}
impl UseCase for Render {
    const NAME: &'static str = "viz.render";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizRenderRequest;
    type Response = VizRenderResponse;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let admitted = self.0.admit(request, caller)?;
        self.0.render(admitted, caller.correlation_id.clone())
    }
}
impl UseCase for Cancel {
    const NAME: &'static str = "viz.cancel";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = VizCancelRequest;
    type Response = VizCancelResponse;
    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }
    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0.cancel(request.target)
    }
}
