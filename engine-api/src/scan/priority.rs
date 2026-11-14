//! Resolves priority requests against a scan's current immutable result.
use super::*;
use du_analysis_index::{QueryError, ResultVersion};
use du_core_contracts::{
    analysis_ipc::JobControlOutcome,
    result::{ResultCompleteness, ResultItemKind, ResultVersionRead},
    scan_priority::{ScanPriorityRequest, ScanPriorityResponse},
};

pub(super) fn register(
    builder: &mut EngineBuilder,
    admitted: Arc<Mutex<Option<Admitted>>>,
    workers: Arc<Workers>,
    store: IngestSink,
) {
    builder.register(PrioritizeScan {
        admitted,
        workers,
        store,
    });
}

struct PrioritizeScan {
    admitted: Arc<Mutex<Option<Admitted>>>,
    workers: Arc<Workers>,
    store: IngestSink,
}

impl UseCase for PrioritizeScan {
    const NAME: &'static str = "scan.prioritize";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ScanPriorityRequest;
    type Response = ScanPriorityResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let mut admitted = self.admitted.lock().map_err(|_| internal())?;
        let Some(active) = admitted
            .as_mut()
            .filter(|active| active.receipt.job_ref == request.job_ref)
        else {
            return Err(EngineError::reason("job.not_found"));
        };

        match self.workers.runtime.state(&request.job_ref) {
            Ok(state) if state.is_terminal() => {
                active.control.take();
                return Ok(already_terminal());
            }
            Ok(_) => {}
            Err(RuntimeError::NotFound) => {
                active.control.take();
                return Ok(already_terminal());
            }
            Err(_) => return Err(internal()),
        }

        let lineage = active.receipt.lineage_id.as_ref().ok_or_else(internal)?;
        let version = self
            .store
            .pin_current(&request.priority_area.result_version)
            .map_err(map_pin_error)?;
        if version.scope_version_id() != &active.receipt.scope_version
            || version.lineage_id() != Some(lineage)
        {
            return Err(EngineError::reason("scan.priority_out_of_scope"));
        }
        if !is_live_scan_partial(&version) {
            active.control.take();
            return Ok(already_terminal());
        }

        let detail = version
            .detail(&request.priority_area)
            .map_err(|_| EngineError::reason("scan.priority_out_of_scope"))?;
        if detail.item.kind != ResultItemKind::Directory {
            return Err(EngineError::reason("scan.priority_out_of_scope"));
        }
        let (root_index, observed_root) = version
            .observed_root_for(&request.priority_area)
            .map_err(|_| EngineError::reason("scan.priority_out_of_scope"))?;
        if root_index as usize >= active.root_count {
            return Err(EngineError::reason("scan.priority_out_of_scope"));
        }
        let (path_root, components) = version
            .observed_path_parts(&request.priority_area)
            .map_err(|_| EngineError::reason("scan.priority_out_of_scope"))?;
        if path_root != observed_root {
            return Err(EngineError::reason("scan.priority_out_of_scope"));
        }
        let components = components.map(<[u8]>::to_vec).collect::<Vec<_>>();

        let Some(control) = active.control.as_ref() else {
            return Ok(already_terminal());
        };
        // Lock order: admitted -> result store -> control attachment -> scheduler queue.
        // The runtime state probe above released its table lock before this guard. Publication
        // takes event-order -> result store; no publication or worker path takes Queue -> result store.
        let outcome = self
            .store
            .with_current(&version, || control.prioritize(root_index, components))
            .map_err(map_pin_error)?;
        match outcome {
            Ok(du_scan_engine::scan::ScanPriorityOutcome::Accepted { .. }) => {
                Ok(ScanPriorityResponse {
                    control_result: JobControlOutcome::Accepted,
                    applied_priority_area: Some(request.priority_area),
                })
            }
            Ok(du_scan_engine::scan::ScanPriorityOutcome::NoEffect {
                same_region: true, ..
            }) => Ok(ScanPriorityResponse {
                control_result: JobControlOutcome::NoEffect,
                applied_priority_area: Some(request.priority_area),
            }),
            Ok(du_scan_engine::scan::ScanPriorityOutcome::NoEffect {
                same_region: false, ..
            }) => Ok(ScanPriorityResponse {
                control_result: JobControlOutcome::NoEffect,
                applied_priority_area: None,
            }),
            Err(du_scan_engine::scan::ScanControlError::AlreadyTerminal) => {
                active.control.take();
                Ok(already_terminal())
            }
            Err(du_scan_engine::scan::ScanControlError::OutOfScope) => {
                Err(EngineError::reason("scan.priority_out_of_scope"))
            }
            Err(du_scan_engine::scan::ScanControlError::NotActive)
            | Err(du_scan_engine::scan::ScanControlError::Unavailable) => Err(internal()),
        }
    }
}

fn map_pin_error(error: QueryError) -> EngineError {
    match error {
        QueryError::StaleVersion | QueryError::NotFound => {
            EngineError::reason("result.version_stale")
        }
        QueryError::Unavailable => internal(),
        _ => EngineError::reason("scan.priority_out_of_scope"),
    }
}

fn already_terminal() -> ScanPriorityResponse {
    ScanPriorityResponse {
        control_result: JobControlOutcome::AlreadyTerminal,
        applied_priority_area: None,
    }
}

fn is_live_scan_partial(version: &ResultVersion) -> bool {
    version.completeness() == ResultCompleteness::Partial
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    use du_core_contracts::{
        PathBytes, PathEncoding,
        result::{ResultNativePath, ResultScopeRoot},
    };

    fn id(value: u8) -> Id {
        Id::from_v7_bytes([value; 16])
    }

    fn version(completeness: ResultCompleteness) -> Result<Arc<ResultVersion>, String> {
        let store = IngestSink::default();
        let lineage = store
            .bind_current_analysis("priority-test-scope", id(1))
            .map_err(|error| format!("bind lineage: {error:?}"))?;
        let root = ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(&[b'C', 0]),
            },
            volume_id: vec![1],
            file_id: vec![2],
        };
        let mut build = store
            .open_forest_in_lineage(id(2), vec![root], lineage)
            .map_err(|error| format!("open scan result: {error:?}"))?;
        build
            .add_directory_at(0, Vec::new())
            .map_err(|error| format!("add root directory: {error:?}"))?;
        build
            .seal(id(3), completeness)
            .map_err(|error| format!("publish scan result: {error:?}"))?;
        store
            .pin_current(&id(3))
            .map_err(|error| format!("pin scan result: {error:?}"))
    }

    #[test]
    fn only_partial_publications_can_admit_live_priority() -> Result<(), String> {
        let partial = version(ResultCompleteness::Partial)?;
        let complete = version(ResultCompleteness::Complete)?;
        assert!(is_live_scan_partial(&partial));
        // A terminal result can be current before JobRuntime records its terminal state.
        assert!(!is_live_scan_partial(&complete));
        Ok(())
    }
}
