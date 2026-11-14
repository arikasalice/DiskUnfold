//! The sample load job: publishes each observation point of a validated bundle as a sample
//! result version.
//!
//! The job reads only the in-memory bundle through the sample source. It never calls
//! `platform-os` and never touches the file system or the network, and it needs no metadata I/O
//! permit. Observation points are collected in bundle order with the shared scan collector; the
//! seal time of each version is the `observedAt` the bundle records for that point, never the
//! current time. A cancel request or any failing point discards every version of the session, so
//! the experience never keeps a partial result.
mod context;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex, PoisonError};

use du_analysis_index::{IngestSink, SampleSessionKey};
use du_core_contracts::{
    Id, Timestamp,
    data_source::DataSource,
    job::{
        Checkpoint, HistoryMode, JobBody, JobContext, JobControl, JobCounts, JobDescriptor,
        JobOutcome, ProgressUnit, ResourceClass,
    },
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    sample_ipc::SAMPLE_LOAD_FAILED,
    scan_observation::ScopeRootPort,
};

use crate::sample_bundle::{SampleObservationState, ValidatedBundle};
use crate::sample_source::{SAMPLE_ROOT_PATH, SampleScope};
use crate::scan::{DevicePlan, ResultIds, collect_open_forest_diagnostic};
use context::SampleContext;

/// Job kind that publishes the sample observations of a validated bundle.
pub const SAMPLE_JOB_KIND: &str = "scan.sample";
/// The sample tree is tiny, so one collector worker is enough and keeps the run deterministic.
const SAMPLE_WORKERS: usize = 1;

/// Describes the cancellable sample load job. It touches no storage and leaves no history.
pub fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: SAMPLE_JOB_KIND.into(),
        version: 1,
        display_name_key: "job.kind.sample_load".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["sample.observation_sealed".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::Compute],
        history: HistoryMode::None,
    }
}

/// How a sample load ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleLoadEnd {
    /// Every observation point was sealed.
    Done,
    /// A cancel request stopped the load; the session's versions were released.
    Cancelled,
    /// A point failed or was incomplete; the session's versions were released.
    Failed,
}

/// One sealed observation point: bundle order, sealed version and the recorded observation time.
pub type SealedObservation = (u32, Id, Option<Timestamp>);

#[derive(Default)]
struct ReportState {
    observations: Vec<SealedObservation>,
    end: Option<SampleLoadEnd>,
    completed_items: u64,
    total_items: u64,
}

/// Result record of one load, shared between the job body and its owner.
#[derive(Default)]
pub struct SampleLoadReport {
    state: Mutex<ReportState>,
}

impl SampleLoadReport {
    /// Sealed observation points in bundle order. Empty after a cancelled or failed load.
    pub fn observations(&self) -> Vec<SealedObservation> {
        self.with(|state| state.observations.clone())
    }

    /// The end state, once the job has finished.
    pub fn end(&self) -> Option<SampleLoadEnd> {
        self.with(|state| state.end)
    }

    /// Items processed so far and the declared total (the progress unit is items).
    pub fn progress(&self) -> (u64, u64) {
        self.with(|state| (state.completed_items, state.total_items))
    }

    fn with<T>(&self, read: impl FnOnce(&mut ReportState) -> T) -> T {
        read(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Where a load publishes: the result store, the owning session and the version identifiers.
pub struct SampleLoadTarget {
    /// Result store that receives the sample versions.
    pub sink: IngestSink,
    /// Session that owns every version of this load.
    pub session: SampleSessionKey,
    /// Scope version the sample builds are bound to.
    pub scope_version_id: Id,
    /// Supplies the identifier of each published version.
    pub ids: Arc<dyn ResultIds>,
}

/// Job body that loads every observation point of a bundle into the result store.
pub struct SampleLoad {
    bundle: Arc<ValidatedBundle>,
    target: SampleLoadTarget,
    report: Arc<SampleLoadReport>,
}

impl SampleLoad {
    /// Binds a validated bundle, its publication target and the report the job fills in.
    pub fn new(
        bundle: Arc<ValidatedBundle>,
        target: SampleLoadTarget,
        report: Arc<SampleLoadReport>,
    ) -> Self {
        Self {
            bundle,
            target,
            report,
        }
    }

    /// The progress total: the item count the bundle header declares.
    pub fn progress_total(&self) -> u64 {
        self.bundle.header().declared_item_count
    }

    fn load(&self, context: &Arc<dyn JobContext>, counts: &mut JobCounts) -> SampleLoadEnd {
        for position in 0..self.bundle.observation_count() {
            if context.checkpoint() == Checkpoint::Cancel {
                return SampleLoadEnd::Cancelled;
            }
            if let Err(end) = self.load_observation(context, position, counts) {
                return end;
            }
        }
        SampleLoadEnd::Done
    }

    fn load_observation(
        &self,
        context: &Arc<dyn JobContext>,
        position: usize,
        counts: &mut JobCounts,
    ) -> Result<(), SampleLoadEnd> {
        let point = self
            .bundle
            .observations()
            .get(position)
            .ok_or(SampleLoadEnd::Failed)?;
        let scope = SampleScope::new(&self.bundle, position).map_err(|_| SampleLoadEnd::Failed)?;
        let opened = scope
            .open_root(SAMPLE_ROOT_PATH)
            .map_err(|_| SampleLoadEnd::Failed)?;
        let root = ResultScopeRoot {
            path: ResultNativePath {
                encoding: opened.canonical_path.encoding,
                raw: opened.canonical_path.raw,
            },
            volume_id: opened.volume_id,
            file_id: opened.file_id,
        };
        let build = self.target.sink.open_sample_forest(
            self.target.scope_version_id.clone(),
            vec![root],
            &self.target.session,
        );
        let sample_context = SampleContext::new(Arc::clone(context), point.observed_at.clone());
        let outcome = collect_open_forest_diagnostic(
            build,
            vec![opened.directory],
            &*self.target.ids,
            &sample_context,
            SAMPLE_WORKERS,
            None,
            (
                DevicePlan {
                    groups: vec![0],
                    limit: 4,
                },
                None,
                None,
            ),
        );
        // The collector also counts the scope root directory, which is not a bundle item.
        let root_dirs = 1;
        counts.completed += outcome.counts.completed.saturating_sub(root_dirs);
        counts.failed += outcome.counts.failed;
        if outcome.failure_reason_key.is_some() {
            return Err(SampleLoadEnd::Failed);
        }
        if !outcome.completed_scope {
            return Err(SampleLoadEnd::Cancelled);
        }
        let id = outcome.result_version_id.ok_or(SampleLoadEnd::Failed)?;
        let version = self
            .target
            .sink
            .pin_current(&id)
            .map_err(|_| SampleLoadEnd::Failed)?;
        // The bundle's own gap examples (denied, vanished, unknown size) are the only
        // accepted reason for a version that is not Complete.
        let expected_gaps = point
            .items
            .iter()
            .filter(|item| {
                matches!(
                    item.state,
                    Some(
                        SampleObservationState::AccessDenied
                            | SampleObservationState::Vanished
                            | SampleObservationState::SizeUnknown
                    )
                )
            })
            .count() as u64;
        let complete = match version.completeness() {
            ResultCompleteness::Complete => outcome.counts.failed == 0,
            ResultCompleteness::Missing => outcome.counts.failed == expected_gaps,
            _ => false,
        };
        if !complete
            || version.data_source() != DataSource::Sample
            || version.observed_at() != Some(&point.observed_at)
        {
            return Err(SampleLoadEnd::Failed);
        }
        let processed =
            (outcome.counts.completed + outcome.counts.failed).saturating_sub(root_dirs);
        self.report.with(|state| {
            state.completed_items += processed;
            state
                .observations
                .push((point.index, id, version.observed_at().cloned()));
        });
        Ok(())
    }

    /// Releases every version of the session so that nothing partial stays reachable.
    fn discard(&self) {
        let _ = self
            .target
            .sink
            .release_sample_session(&self.target.session);
        self.report.with(|state| state.observations.clear());
    }
}

impl JobBody for SampleLoad {
    fn kind_id(&self) -> &str {
        SAMPLE_JOB_KIND
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let total = self.progress_total();
        self.report.with(|state| state.total_items = total);
        let mut counts = JobCounts::default();
        let end = self.load(&context, &mut counts);
        if end != SampleLoadEnd::Done {
            self.discard();
            counts.unprocessed = total.saturating_sub(counts.completed + counts.failed);
        }
        self.report.with(|state| state.end = Some(end));
        let unprocessed = counts.unprocessed;
        JobOutcome {
            counts,
            completed_scope: end == SampleLoadEnd::Done,
            failure_reason_key: (end == SampleLoadEnd::Failed).then(|| SAMPLE_LOAD_FAILED.into()),
            unprocessed_summary: (end != SampleLoadEnd::Done)
                .then(|| format!("unprocessed_items={unprocessed}")),
            result_version_id: None,
        }
    }
}
