//! GUI-only replacement of an owned scan's resource condition.
use super::*;
use du_core_contracts::analysis_ipc::{
    ScanCollection, ScanSetResourceOutcome, ScanSetResourceRequest, ScanSetResourceResponse,
};

pub(super) fn register(
    builder: &mut EngineBuilder,
    admitted: Arc<Mutex<Option<Admitted>>>,
    workers: Arc<Workers>,
) {
    builder.register(SetScanResource { admitted, workers });
}

struct SetScanResource {
    admitted: Arc<Mutex<Option<Admitted>>>,
    workers: Arc<Workers>,
}

impl UseCase for SetScanResource {
    const NAME: &'static str = "scan.set_resource";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ScanSetResourceRequest;
    type Response = ScanSetResourceResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        if !du_scan_engine::scan::valid_resource_limit(request.user_limit) {
            return Err(EngineError::reason("scan.resource_limit_invalid"));
        }
        let mut admitted = self.admitted.lock().map_err(|_| internal())?;
        let snapshot = self
            .workers
            .runtime
            .snapshot(&request.job_ref)
            .map_err(runtime_error)?;
        if snapshot.kind_id != "scan.metadata" {
            return Err(EngineError::reason("job.control_unsupported"));
        }
        let Some(active) = admitted
            .as_mut()
            .filter(|active| active.receipt.job_ref == request.job_ref)
        else {
            return Err(EngineError::reason("job.not_found"));
        };
        let Some(control) = active.control.as_ref() else {
            return Err(EngineError::reason("job.control_unsupported"));
        };
        let (outcome, lanes) = control
            .set_resource(request.resource_mode, request.user_limit)
            .map_err(|error| match error {
                du_scan_engine::scan::ScanControlError::AlreadyTerminal => internal(),
                du_scan_engine::scan::ScanControlError::Unavailable => internal(),
                du_scan_engine::scan::ScanControlError::NotActive
                | du_scan_engine::scan::ScanControlError::OutOfScope => internal(),
            })?;
        if outcome == ScanSetResourceOutcome::Accepted {
            active.receipt.collection = Some(ScanCollection {
                resource_mode: request.resource_mode,
                user_limit: request.user_limit,
            });
        }
        if outcome == ScanSetResourceOutcome::AlreadyTerminal {
            active.control.take();
        }
        Ok(ScanSetResourceResponse {
            outcome,
            lanes,
            applies_from: "next_assignment".into(),
        })
    }
}

fn runtime_error(error: RuntimeError) -> EngineError {
    EngineError::reason(match error {
        RuntimeError::NotFound => "job.not_found",
        _ => "support.internal_error",
    })
}
