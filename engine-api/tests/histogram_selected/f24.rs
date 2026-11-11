use super::*;
use du_core_contracts::job::{
    HistoryAppend, HistoryMode, JobBody, JobContext, JobControl, JobDescriptor, JobHistory,
    JobOutcome, JobSummary, ProgressUnit, ResourceClass,
};
use du_job_runtime::{
    JobClock, JobEvent, JobEvents, JobPriority, JobRuntime, Submission, SubmitResult,
};
use std::sync::Mutex;
struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        0
    }
}
struct History;
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}
struct Events;
impl JobEvents for Events {
    fn publish(&self, _: JobEvent) {}
}
struct Body;
impl JobBody for Body {
    fn kind_id(&self) -> &str {
        "viz.compute"
    }
    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        JobOutcome {
            counts: Default::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
fn candidate(
    version: &Arc<ResultVersion>,
    tag: &str,
) -> Result<histogram::SelectedCandidate, String> {
    let scope = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let d = dispatcher()?;
    let selected = PinnedRendered::compute(
        version.clone(),
        histogram_dispatch::Request {
            version,
            scope: &scope,
            size_basis: SizeBasis::Logical,
            unit_table: &SI_UNITS,
            unit_table_revision: UNIT_TABLE_REVISION,
            bounds: bounds(),
            basis: Basis::ItemCount,
            scale: Scale::Automatic,
            unknown_reason: "fixture.unknown",
        },
        &d,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let bins = selected.rendered().prepared().unit_bins().bins.clone();
    histogram::prepare_selected(
        histogram::Request {
            version: version.clone(),
            scope: &scope,
            size_basis: SizeBasis::Logical,
            bins: &bins,
            bounds: bounds(),
            scale: Scale::Automatic,
            fingerprint: tag.into(),
            unknown_reason: "fixture.unknown".into(),
            bin_presentation: (0..bins.len())
                .map(|i| BinPresentation {
                    group_ref: format!("g{i}"),
                    display_name: format!("bin {i}"),
                    legend_class_key: "c".into(),
                    display_flags: vec![],
                })
                .collect(),
            item_presentation: (1..=2)
                .map(|n| RowPresentation {
                    item: ItemRef {
                        result_version: id(2),
                        item_no: n,
                    },
                    legend_class_key: "c".into(),
                    display_flags: vec![],
                })
                .collect(),
        },
        selected,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))
}
fn runtime() -> Result<Arc<JobRuntime>, String> {
    let runtime = Arc::new(
        JobRuntime::new(
            Arc::new(Ids(AtomicU8::new(50))),
            Arc::new(Clock),
            Arc::new(History),
            Arc::new(Events),
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    runtime
        .register(JobDescriptor {
            kind_id: "viz.compute".into(),
            version: 1,
            display_name_key: "viz.compute".into(),
            controls: vec![JobControl::Cancel],
            safe_points: vec!["segment".into()],
            progress_unit: ProgressUnit::Items,
            resource_classes: vec![ResourceClass::Compute],
            history: HistoryMode::None,
        })
        .map_err(|e| format!("{e:?}"))?;
    Ok(runtime)
}
#[test]
fn job_gate_commits_or_withdraws_both_pages_and_frame() -> Result<(), String> {
    let sink = IngestSink::default();
    let version = publish(&sink, 2)?;
    let pages = Arc::new(ModelPages::new(
        sink.clone(),
        NonZeroUsize::MIN.saturating_add(3),
        Arc::new(Ids(AtomicU8::new(10))),
    ));
    let runtime = runtime()?;
    for mode in ["done", "cancel", "replaced"] {
        let deferred = Arc::new(Mutex::new(
            pages
                .defer_histogram_pages(candidate(&version, mode)?, &mut || false)
                .map_err(|e| format!("{e:?}"))?,
        ));
        let reference = deferred
            .lock()
            .map_err(|_| "lock")?
            .reserved_reference()
            .to_owned();
        let worker_deferred = deferred.clone();
        let worker_pages = pages.clone();
        let submitted = runtime
            .submit_viz_with_retained_admission(
                Submission {
                    kind_id: "viz.compute".into(),
                    deduplication_key: None,
                    body: Arc::new(Body),
                    priority: JobPriority::Normal,
                    scope_summary: "test".into(),
                    correlation_id: id(6),
                    sample: false,
                },
                deferred.clone(),
                move || {
                    let mut transaction = worker_deferred.lock().map_err(|_| "lock".to_owned())?;
                    worker_pages
                        .commit_histogram_pages(&mut transaction, &mut || false)
                        .map(|_| ())
                        .map_err(|e| format!("{e:?}"))
                },
                |_| Ok(()),
            )
            .map_err(|e| format!("{e:?}"))?;
        let SubmitResult::Created(job) = submitted else {
            return Err("unexpected reuse".into());
        };
        assert!(pages.histogram_frame(&reference).is_err());
        if mode == "cancel" {
            runtime.cancel(&job).map_err(|e| format!("{e:?}"))?;
        }
        if mode == "replaced" {
            let _ = publish(&sink, 3)?;
        }
        runtime.run_next().map_err(|e| format!("{e:?}"))?;
        let frame = pages.histogram_frame(&reference);
        let rows = pages.rows(&VizRowsRequest {
            model_ref: reference,
            result_revision: id(2),
            start: 0,
            count: 1,
        });
        assert_eq!(frame.is_ok(), mode == "done");
        assert_eq!(rows.is_ok(), mode == "done");
    }
    Ok(())
}
