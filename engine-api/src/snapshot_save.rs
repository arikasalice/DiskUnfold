//! Admit and run one immutable result version's local snapshot save.

mod errors;
#[cfg(test)]
mod tests;

use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

use du_analysis_index::{IngestSink, ResultVersion};
use du_core_contracts::{
    ENVELOPE_VERSION, EventEnvelope, Id, Timestamp,
    data_source::DataSource,
    job::{
        Checkpoint, HistoryMode, JobBody, JobContext, JobControl, JobCounts, JobDescriptor,
        JobOutcome, JobProgress, ProgressUnit, ResourceClass,
    },
    result::ResultVersionRead,
    snapshot_file::SnapshotFilePort,
    snapshot_index::{
        SnapshotIndexPort, SnapshotIndexRow, SnapshotIndexSourceKind, SnapshotIndexState,
        SnapshotIndexVolume, SnapshotVolumeIdentityCapability, SnapshotVolumeIdentityKind,
    },
    snapshot_ipc::{
        SnapshotChange, SnapshotChangedBody, SnapshotPinApplied, SnapshotSaveAccepted,
        SnapshotSaveAcceptedOutcome, SnapshotSaveExistingJob, SnapshotSaveExistingOutcome,
        SnapshotSaveExistingSnapshot, SnapshotSaveRequest, SnapshotSaveResponse,
    },
};
use du_job_runtime::{
    DeduplicationKey, JobPriority, JobRuntime, RuntimeError, Submission, SubmitResult,
};
use du_snapshot_store::serializer::{
    AlgorithmVersion, CaseSensitivity, ConditionSummary, Normalization, PathRule, PathSeparator,
    SnapshotIdentity, SnapshotWriteReport, SourceMetadata, write_v1_cancellable,
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    desktop::DesktopServices,
    error::EngineError,
};
use errors::{
    completed_error, file_error, index_error, publish_error, result_error, runtime_error,
    save_file_error, serializer_error,
};

const KIND: &str = "snapshot.save";
const REQUIRED_FIXED_SPACE: u128 = 256 * 1024 * 1024;

/// Registers the GUI-only save command and its background job kind.
impl DesktopServices {
    /// Registers snapshot saving with host-owned index and file adapters.
    pub fn register_snapshot_save<I>(
        &self,
        builder: &mut EngineBuilder,
        index: Arc<Mutex<I>>,
        files: Arc<dyn SnapshotFilePort>,
        primary_window: bool,
    ) -> Result<(), RuntimeError>
    where
        I: SnapshotIndexPort + Send + 'static,
    {
        let descriptor = JobDescriptor {
            kind_id: KIND.into(),
            version: 1,
            display_name_key: "job.snapshot_save".into(),
            controls: vec![JobControl::Cancel, JobControl::SetPriority],
            safe_points: vec!["batch_end".into()],
            progress_unit: ProgressUnit::Bytes,
            resource_classes: vec![ResourceClass::Output],
            history: HistoryMode::Summary,
        };
        self.runtime.register(descriptor)?;
        let context = Arc::new(SaveContext {
            store: self.store.clone(),
            index,
            files,
            queue: Arc::new(SaveQueue {
                runtime: self.runtime.clone(),
                workers: self.workers.clone(),
                events: self.events.clone(),
            }),
        });
        builder.register(SaveCommand {
            context,
            primary_window,
        });
        Ok(())
    }
}

struct SaveContext<I> {
    store: IngestSink,
    index: Arc<Mutex<I>>,
    files: Arc<dyn SnapshotFilePort>,
    queue: Arc<SaveQueue>,
}

struct SaveQueue {
    runtime: Arc<JobRuntime>,
    workers: Arc<crate::workers::Workers>,
    events: Arc<crate::desktop::DesktopEvents>,
}

struct SaveCommand<I> {
    context: Arc<SaveContext<I>>,
    primary_window: bool,
}

impl<I> UseCase for SaveCommand<I>
where
    I: SnapshotIndexPort + Send + 'static,
{
    const NAME: &'static str = "snapshot.save";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SnapshotSaveRequest;
    type Response = SnapshotSaveResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        if self.primary_window {
            Ok(())
        } else {
            Err(EngineError::reason("command.window_denied"))
        }
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let version = self
            .context
            .store
            .pin_current(&request.result_version_id)
            .map_err(result_error)?;
        if version.data_source() == DataSource::Sample {
            return Err(EngineError::reason("snapshot.sample_source"));
        }
        let completed = self
            .context
            .store
            .pin_completed_scan(&request.result_version_id)
            .map_err(completed_error)?;
        if completed.version().version_id() != version.version_id()
            || !matches!(
                completed.completion(),
                du_analysis_index::ScanCompletion::Done
                    | du_analysis_index::ScanCompletion::Partial
            )
        {
            return Err(EngineError::reason("snapshot.result_not_complete"));
        }
        let lineage_id = version
            .lineage_id()
            .cloned()
            .ok_or_else(|| EngineError::reason("snapshot.result_not_complete"))?;
        let key = deduplication_key(version.version_id());
        if let Some(snapshot_id) = self
            .context
            .index
            .lock()
            .map_err(|_| EngineError::reason("snapshot.store_unavailable"))?
            .find_active_by_source(version.version_id())
            .map_err(index_error)?
        {
            return Ok(existing_snapshot(snapshot_id));
        }
        if let Some(job_id) = self
            .context
            .queue
            .runtime
            .find_active_deduplicated(KIND, &key)
            .map_err(runtime_error)?
        {
            return Ok(existing_job(job_id));
        }

        if let Some(available) = self.context.files.available_bytes()
            && u128::from(available) < required_free_bytes(version.items().len())
        {
            eprintln!("snapshot.insufficient_space");
            return Err(EngineError::reason("snapshot.store_unavailable"));
        }
        let snapshot_id = Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes());
        let pending = self
            .context
            .files
            .create_pending(&snapshot_id)
            .map_err(file_error)?;
        drop(pending);

        let body: Arc<dyn JobBody> = Arc::new(SaveJob {
            context: self.context.clone(),
            version,
            source_version_id: request.result_version_id.clone(),
            lineage_id,
            snapshot_id,
            pin: request.pin,
        });
        let submission = Submission {
            kind_id: KIND.into(),
            deduplication_key: Some(key),
            body,
            priority: JobPriority::Normal,
            scope_summary: "snapshot.save".into(),
            scope_roots: Vec::new(),
            correlation_id: caller.correlation_id.clone(),
            sample: false,
        };
        match self
            .context
            .queue
            .runtime
            .submit(submission)
            .map_err(runtime_error)?
        {
            SubmitResult::Created(job_id) => {
                self.context.queue.workers.notify();
                Ok(SnapshotSaveResponse::Accepted(SnapshotSaveAccepted {
                    outcome: SnapshotSaveAcceptedOutcome::Accepted,
                    job_id,
                }))
            }
            SubmitResult::Existing(job_id) => Ok(existing_job(job_id)),
        }
    }
}

struct SaveJob<I> {
    context: Arc<SaveContext<I>>,
    version: Arc<ResultVersion>,
    source_version_id: Id,
    lineage_id: Id,
    snapshot_id: Id,
    pin: bool,
}

impl<I> JobBody for SaveJob<I>
where
    I: SnapshotIndexPort + Send + 'static,
{
    fn kind_id(&self) -> &str {
        KIND
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        match self.save(context.clone()) {
            Ok(()) => outcome(
                true,
                JobCounts {
                    completed: 1,
                    ..JobCounts::default()
                },
                None,
            ),
            Err(SaveFailure::Cancelled) => outcome(false, JobCounts::default(), None),
            Err(SaveFailure::Failed(reason)) => outcome(false, JobCounts::default(), Some(reason)),
        }
    }
}

impl<I> SaveJob<I>
where
    I: SnapshotIndexPort + Send + 'static,
{
    fn save(&self, context: Arc<dyn JobContext>) -> Result<(), SaveFailure> {
        let created_at = context
            .utc_now()
            .or_else(current_timestamp)
            .ok_or(SaveFailure::Failed("snapshot.write_failed"))?;
        let mut pending = self
            .context
            .files
            .create_pending(&self.snapshot_id)
            .map_err(save_file_error)?;
        let source = source_metadata(&self.version);
        let report = {
            let mut output = ProgressWriter {
                output: &mut *pending,
                context: context.clone(),
                completed: 0,
            };
            let result = write_v1_cancellable(
                &self.version,
                SnapshotIdentity {
                    snapshot_id: &self.snapshot_id,
                    created_at: &created_at,
                    app_version: env!("CARGO_PKG_VERSION"),
                },
                &source,
                &mut output,
                || context.checkpoint() == Checkpoint::Cancel,
            );
            result.map_err(serializer_error)?
        };
        if context.checkpoint() == Checkpoint::Cancel {
            return Err(SaveFailure::Cancelled);
        }
        pending.publish().map_err(publish_error)?;
        let row = index_row(self, &created_at, &report)
            .ok_or(SaveFailure::Failed("snapshot.write_failed"))?;
        let volumes = index_volumes(&report).ok_or(SaveFailure::Failed("snapshot.write_failed"))?;
        let registration = self
            .context
            .index
            .lock()
            .map_err(|_| SaveFailure::Failed("snapshot.write_failed"))?
            .register(&row, &volumes, self.pin.then_some(created_at.unix_millis()))
            .map_err(|_| SaveFailure::Failed("snapshot.write_failed"))?;
        if registration.inserted {
            self.context.queue.events.publish_host_event(EventEnvelope {
                topic: "snapshot.changed".into(),
                contract_version: ENVELOPE_VERSION,
                seq: 0,
                correlation_id: None,
                job_ref: None,
                result_version: Some(self.source_version_id.clone()),
                occurred_at: created_at.clone(),
                body: serde_json::to_value(SnapshotChangedBody {
                    list_seq: registration.list_seq,
                    change: SnapshotChange::Saved,
                    snapshot_ids: vec![self.snapshot_id.clone()],
                    observed_at: self.version.observed_at().cloned(),
                })
                .map_err(|_| SaveFailure::Failed("snapshot.write_failed"))?,
            });
        }
        Ok(())
    }
}

struct ProgressWriter<'a> {
    output: &'a mut dyn Write,
    context: Arc<dyn JobContext>,
    completed: u64,
}

impl Write for ProgressWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.output.write(bytes)?;
        self.completed = self.completed.saturating_add(written as u64);
        self.context.report_progress(JobProgress {
            completed: self.completed,
            total: None,
        });
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

enum SaveFailure {
    Cancelled,
    Failed(&'static str),
}

fn source_metadata(version: &ResultVersion) -> SourceMetadata {
    let path_rules = version
        .observed_roots()
        .iter()
        .map(|root| PathRule {
            separator: match root.path.encoding {
                du_core_contracts::PathEncoding::Unix => PathSeparator::Slash,
                du_core_contracts::PathEncoding::WindowsUtf16le => PathSeparator::Backslash,
            },
            case_sensitivity: None::<CaseSensitivity>,
            normalization: None::<Normalization>,
        })
        .collect();
    let size_rule = version.size_rule();
    SourceMetadata {
        condition_summary: ConditionSummary::Unknown,
        path_rules,
        algorithms: vec![AlgorithmVersion {
            kind: "size".into(),
            id: size_rule.id.into(),
            version: size_rule.revision,
        }],
    }
}

fn index_row<I>(
    job: &SaveJob<I>,
    created_at: &Timestamp,
    report: &SnapshotWriteReport,
) -> Option<SnapshotIndexRow> {
    let body_size = i64::try_from(report.body_size).ok()?;
    Some(SnapshotIndexRow {
        snapshot_id: job.snapshot_id.clone(),
        file_name: format!("{}.dusnap", job.snapshot_id.as_str()),
        source_version_id: job.source_version_id.clone(),
        lineage_id: job.lineage_id.clone(),
        source_kind: SnapshotIndexSourceKind::Local,
        observed_at_ms: job.version.observed_at().map(Timestamp::unix_millis),
        created_at_ms: created_at.unix_millis(),
        scope_label: report.scope_marker.clone(),
        condition_summary: report.condition_summary_json.clone(),
        completeness: Some(report.completeness),
        format_version: report.format_version,
        body_size,
        body_hash: report.body_hash,
        state: SnapshotIndexState::Active,
        verify_failed_at_ms: None,
        updated_at_ms: created_at.unix_millis(),
        row_version: 1,
    })
}

fn index_volumes(report: &SnapshotWriteReport) -> Option<Vec<SnapshotIndexVolume>> {
    report
        .volume_ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            Some(SnapshotIndexVolume {
                volume_index: u16::try_from(index).ok()?,
                observed_volume_id: (!id.is_empty()).then(|| id.clone()),
                identity_kind: SnapshotVolumeIdentityKind::None,
                identity_capability: SnapshotVolumeIdentityCapability::Unknown,
            })
        })
        .collect()
}

fn required_free_bytes(item_count: usize) -> u128 {
    (item_count as u128 * 504).div_ceil(10) + REQUIRED_FIXED_SPACE
}

fn deduplication_key(version_id: &Id) -> DeduplicationKey {
    DeduplicationKey {
        scope: "snapshot.save".into(),
        input: version_id.as_str().into(),
        channel: "gui".into(),
        key: version_id.as_str().into(),
    }
}

fn existing_snapshot(snapshot_id: Id) -> SnapshotSaveResponse {
    SnapshotSaveResponse::ExistingSnapshot(SnapshotSaveExistingSnapshot {
        outcome: SnapshotSaveExistingOutcome::Existing,
        snapshot_id,
        pin_applied: SnapshotPinApplied::FALSE,
    })
}

fn existing_job(job_id: Id) -> SnapshotSaveResponse {
    SnapshotSaveResponse::ExistingJob(SnapshotSaveExistingJob {
        outcome: SnapshotSaveExistingOutcome::Existing,
        job_id,
        pin_applied: SnapshotPinApplied::FALSE,
    })
}

fn outcome(completed: bool, counts: JobCounts, failure: Option<&'static str>) -> JobOutcome {
    JobOutcome {
        counts,
        completed_scope: completed,
        failure_reason_key: failure.map(str::to_owned),
        unprocessed_summary: None,
        result_version_id: None,
    }
}

fn current_timestamp() -> Option<Timestamp> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    Timestamp::from_unix_millis(i64::try_from(millis).ok()?)
}
