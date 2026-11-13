//! Dispatches the two GUI-only controls for cooperative scan suspension.
use super::*;

pub(super) struct Suspension<const PAUSE: bool>(pub(super) Arc<JobRuntime>);

impl<const PAUSE: bool> UseCase for Suspension<PAUSE> {
    const NAME: &'static str = if PAUSE { "job.pause" } else { "job.resume" };
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = AnalysisJobRequest;
    type Response = AnalysisJobControl;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let result = if PAUSE {
            self.0.pause(&request.job_ref)
        } else {
            self.0.resume(&request.job_ref)
        };
        control_response(result)
    }
}
