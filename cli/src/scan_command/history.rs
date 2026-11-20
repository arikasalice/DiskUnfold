//! Defers CLI history persistence until the scan's terminal status is observed.

use std::{
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use du_core_contracts::{
    Id,
    job::{HistoryAppend, JobHistory, JobSummary},
};
use du_platform_os::interrupt::Interrupts;

#[derive(Default)]
pub(crate) struct DeferredHistory(Mutex<Option<JobSummary>>);

impl JobHistory for DeferredHistory {
    fn append(&self, summary: &JobSummary) -> Result<HistoryAppend, String> {
        let mut pending = self
            .0
            .lock()
            .map_err(|_| "history.not_recorded".to_owned())?;
        if pending.is_some() {
            return Err("history.not_recorded".to_owned());
        }
        *pending = Some(summary.clone());
        Err("history.not_recorded".to_owned())
    }
}

pub(super) enum Outcome {
    Recorded,
    Refused,
    Busy,
    DiskFull,
    Interrupted,
}

pub(super) fn record(
    history: &DeferredHistory,
    expected_job: &Id,
    data_directory: Option<&Path>,
    interrupts: Option<Interrupts>,
    interrupt_count: usize,
) -> Outcome {
    let Ok(mut pending) = history.0.lock() else {
        return Outcome::Refused;
    };
    let Some(summary) = pending.take() else {
        return Outcome::Refused;
    };
    if &summary.job_id != expected_job {
        return Outcome::Refused;
    }
    let Some(directory) = data_directory else {
        return Outcome::Refused;
    };
    let path = directory.join(du_platform_os::application_data::DATABASE_FILE);
    let mut database = match du_app_db::AppDb::open_existing_for_history(&path) {
        Ok(database) => database,
        Err(du_app_db::HistoryOpenError::Busy) => return Outcome::Busy,
        Err(du_app_db::HistoryOpenError::DiskFull) => return Outcome::DiskFull,
        Err(du_app_db::HistoryOpenError::Refused(_) | du_app_db::HistoryOpenError::Other(_)) => {
            return Outcome::Refused;
        }
    };
    let Some(now_ms) = now_ms() else {
        return Outcome::Refused;
    };
    let appended = database.append_scan_history_interruptible(&summary, now_ms, move || {
        interrupts.is_some_and(|state| state.count() > interrupt_count)
    });
    let _ = database.close();
    match appended {
        Ok(_) => Outcome::Recorded,
        Err(reason) if reason == "db.busy" => Outcome::Busy,
        Err(reason) if reason == "db.disk_full" => Outcome::DiskFull,
        Err(reason) if reason == "history.interrupted" => Outcome::Interrupted,
        Err(_) => Outcome::Refused,
    }
}

fn now_ms() -> Option<i64> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(duration.as_millis()).ok()
}
