//! Bind the desktop job runtime and worker pool to engine shutdown hooks.

use std::sync::Arc;

use du_job_runtime::JobRuntime;

use crate::{
    shutdown::{
        ShutdownAdmission, ShutdownHooks, ShutdownJobCancellation, ShutdownPreparation,
        WatchStopResult,
    },
    workers::Workers,
};

pub(super) fn hooks(runtime: Arc<JobRuntime>, workers: Arc<Workers>) -> ShutdownHooks {
    let admission_runtime = runtime.clone();
    let prepare_runtime = runtime.clone();
    let history_runtime = runtime;
    ShutdownHooks::new(
        move |confirmed| {
            let admission = admission_runtime.close_admission_for_shutdown(confirmed)?;
            Ok(match admission {
                du_job_runtime::AdmissionClose::Accepted { active_change_jobs } => {
                    ShutdownAdmission::Accepted { active_change_jobs }
                }
                du_job_runtime::AdmissionClose::ConfirmRequired { active_change_jobs } => {
                    ShutdownAdmission::ConfirmRequired { active_change_jobs }
                }
                du_job_runtime::AdmissionClose::AlreadyClosed => ShutdownAdmission::AlreadyClosing,
            })
        },
        move || {
            let cancellations = prepare_runtime
                .cancel_all_for_shutdown()?
                .into_iter()
                .map(|(job_ref, outcome)| ShutdownJobCancellation { job_ref, outcome })
                .collect();
            Ok(ShutdownPreparation {
                // No OS watcher is installed by the current desktop composition.
                watch_stop: WatchStopResult::NotInstalled,
                cancellations,
            })
        },
        move |timeout| history_runtime.begin_shutdown(timeout),
        move || workers.close_and_join(),
    )
}
