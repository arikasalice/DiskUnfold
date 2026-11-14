//! `group.classify`, `group.summary` and `group.cancel`.
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use du_analysis_index::{
    IngestSink, QueryError, ResultVersion,
    group::{self, Classification, Control, Parallelism, ruleset},
    query::{Condition, Registry},
};
use du_core_contracts::{
    Id, ItemRef,
    data_source::DataSource,
    group_ipc::{GroupClassifyRequest, GroupClassifyResponse, GroupRuleSource},
    job::{Checkpoint, JobBody, JobContext, JobOutcome, JobProgress},
    result::ResultVersionRead,
};
use du_job_runtime::{DeduplicationKey, JobPriority, RuntimeError, Submission, SubmitResult};

use super::{JOB_KIND, Meta, Services, codec, hex, internal, summary::summary_response, tables};
use crate::{
    api::{CallerCtx, Channel, UseCase},
    browse::folder::lower_condition,
    error::EngineError,
};

/// Provisional synchronous wait (F36 DLD Q3) before work moves to a job.
const SYNC_LIMIT: Duration = Duration::from_millis(500);
/// Scopes larger than this go straight to a job.
const SYNC_FILES: u64 = 200_000;

/// Test-only view that always takes the job path.
#[cfg(test)]
pub(super) const FORCE_JOB_VIEW: &str = "018f3333-0000-7000-8000-00000000f00b";

fn sync_allowed(view: &Id, files: u64) -> bool {
    #[cfg(test)]
    if view.as_str() == FORCE_JOB_VIEW {
        return false;
    }
    let _ = view;
    files <= SYNC_FILES
}

/// Rules fixed at admission.
struct Resolved {
    conditions: Vec<Condition>,
    group_ids: Vec<Id>,
    names: Vec<String>,
    ruleset_version: u32,
    fingerprint: [u8; 32],
}

pub(super) struct Classify(pub(super) Arc<Services>);

impl UseCase for Classify {
    const NAME: &'static str = "group.classify";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = GroupClassifyRequest;
    type Response = GroupClassifyResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let services = &self.0;
        let seq = request.request_seq;
        let resolved = resolve(services, request.rules)?;
        let version = pin(&services.assembly.store, request.result_version.as_ref())?;
        if !tables::source_allowed(version.origin()) {
            return Ok(GroupClassifyResponse::NotApplicable {
                request_seq: seq,
                reason_key: "group.source_unsupported".into(),
            });
        }
        if let Some(scope) = &request.scope
            && &scope.result_version != version.version_id()
        {
            return Err(EngineError::reason("cursor.invalid"));
        }
        let view = request.view_ref;
        let flag = services
            .channels
            .admit(&view, seq, &services.assembly.runtime)?;
        let meta = Arc::new(meta(
            &version,
            request.scope.as_ref(),
            resolved.clone_meta(),
        ));
        let store = &services.assembly.store;
        if let Some(found) = services.registered.find(&meta.key, store) {
            services.channels.finish(&view, seq);
            return summary_response(services, seq, &found);
        }
        let classification_ref = services.assembly.ids.next_id();
        let files =
            group::scope_file_count(&version, request.scope.as_ref()).map_err(classify_error)?;
        let admission = Admission {
            version,
            scope: request.scope,
            conditions: resolved.conditions,
            meta,
            classification_ref,
            files,
        };
        if sync_allowed(&view, files) {
            let control = SyncControl {
                caller,
                flag: &flag,
                deadline: Instant::now() + SYNC_LIMIT,
                expired: AtomicBool::new(false),
            };
            match run(&admission, &services.registry, &control) {
                Ok(classification) => {
                    services.registered.register(
                        admission.classification_ref.clone(),
                        classification,
                        admission.meta.clone(),
                    )?;
                    services.channels.finish(&view, seq);
                    return summary_response(services, seq, &admission.classification_ref);
                }
                Err(group::Error::Cancelled) if control.timed_out() => {}
                Err(error) => {
                    services.channels.finish(&view, seq);
                    return Err(classify_error(error));
                }
            }
        }
        submit(services, caller, admission, (&view, seq))
    }
}

impl Resolved {
    fn clone_meta(&self) -> (Vec<Id>, Vec<String>, u32, String) {
        (
            self.group_ids.clone(),
            self.names.clone(),
            self.ruleset_version,
            hex(&self.fingerprint),
        )
    }
}

struct Admission {
    version: Arc<ResultVersion>,
    scope: Option<ItemRef>,
    conditions: Vec<Condition>,
    meta: Arc<Meta>,
    classification_ref: Id,
    /// Files in the scope; the confirmed progress total of a job.
    files: u64,
}

fn run(
    admission: &Admission,
    registry: &Registry,
    control: &dyn Control,
) -> Result<Classification, group::Error> {
    group::classify_scope(
        admission.version.clone(),
        admission.scope.as_ref(),
        registry,
        admission.conditions.clone(),
        Parallelism::default(),
        control,
    )
}

fn resolve(services: &Services, source: GroupRuleSource) -> Result<Resolved, EngineError> {
    match source {
        GroupRuleSource::Saved { ruleset_version } => {
            let loaded = codec::load(services.rules.as_ref())?;
            if loaded.ruleset.version_no != ruleset_version {
                return Err(EngineError::reason("group.rules_modified"));
            }
            let rules = loaded.ruleset.rules;
            Ok(Resolved {
                conditions: rules.iter().map(|rule| rule.condition.clone()).collect(),
                group_ids: rules.iter().map(|rule| rule.group_id.clone()).collect(),
                names: rules.iter().map(|rule| rule.display_name.clone()).collect(),
                ruleset_version,
                fingerprint: loaded.ruleset.fingerprint,
            })
        }
        GroupRuleSource::Draft { rules: drafts } => {
            let mut rules = Vec::with_capacity(drafts.len());
            for draft in drafts {
                let condition = lower_condition(draft.condition)
                    .map_err(|_| EngineError::reason("query.syntax_invalid"))?
                    .normalize();
                rules.push(ruleset::Rule {
                    group_id: draft
                        .group_id
                        .unwrap_or_else(|| services.assembly.ids.next_id()),
                    display_name: ruleset::normalize_name(&draft.display_name),
                    predicates: ruleset::strategies(&condition),
                    condition,
                });
            }
            Ok(Resolved {
                fingerprint: ruleset::fingerprint(&rules),
                conditions: rules.iter().map(|rule| rule.condition.clone()).collect(),
                group_ids: rules.iter().map(|rule| rule.group_id.clone()).collect(),
                names: rules.into_iter().map(|rule| rule.display_name).collect(),
                ruleset_version: 0,
            })
        }
    }
}

fn meta(
    version: &ResultVersion,
    scope: Option<&ItemRef>,
    (group_ids, names, ruleset_version, fingerprint): (Vec<Id>, Vec<String>, u32, String),
) -> Meta {
    let ids: Vec<u8> = group_ids
        .iter()
        .flat_map(|id| id.as_str().as_bytes().to_vec())
        .collect();
    // Duplicate key: result version, scope, rule content and group identities.
    let key = format!(
        "{}|{}|{}|{}",
        version.version_id().as_str(),
        scope.map_or_else(|| "*".to_owned(), |item| item.item_no.to_string()),
        fingerprint,
        hex(blake3::hash(&ids).as_bytes()),
    );
    Meta {
        group_ids,
        names,
        ruleset_version,
        fingerprint,
        key,
    }
}

fn pin(store: &IngestSink, id: Option<&Id>) -> Result<Arc<ResultVersion>, EngineError> {
    match id {
        Some(id) => store.pin_current(id).map_err(|error| match error {
            QueryError::StaleVersion => EngineError::reason("result.version_stale"),
            QueryError::NotFound => EngineError::reason("result.version_not_found"),
            _ => internal(),
        }),
        None => store
            .pin()
            .map_err(|_| internal())?
            .ok_or_else(|| EngineError::reason("result.version_not_found")),
    }
}

fn classify_error(error: group::Error) -> EngineError {
    match error {
        group::Error::InvalidRoot => EngineError::reason("cursor.invalid"),
        group::Error::Source(QueryError::StaleVersion) => {
            EngineError::reason("result.version_stale")
        }
        group::Error::Source(QueryError::NotFound) => EngineError::reason("ref.unavailable"),
        group::Error::InvalidRule(_) => EngineError::reason("query.syntax_invalid"),
        _ => internal(),
    }
}

struct SyncControl<'a> {
    caller: &'a CallerCtx,
    flag: &'a AtomicBool,
    deadline: Instant,
    expired: AtomicBool,
}

impl SyncControl<'_> {
    fn timed_out(&self) -> bool {
        self.expired.load(Ordering::Acquire)
            && !self.caller.is_cancelled()
            && !self.flag.load(Ordering::Acquire)
    }
}

impl Control for SyncControl<'_> {
    fn cancelled(&self) -> bool {
        if Instant::now() >= self.deadline {
            self.expired.store(true, Ordering::Release);
            return true;
        }
        self.caller.is_cancelled() || self.flag.load(Ordering::Acquire)
    }

    fn progress(&self, _: u64) {}
}

fn submit(
    services: &Services,
    caller: &CallerCtx,
    admission: Admission,
    (view, seq): (&Id, u32),
) -> Result<GroupClassifyResponse, EngineError> {
    let slot = Arc::new(Mutex::new(None));
    let key = admission.meta.key.clone();
    let version_id = admission.version.version_id().clone();
    let data_source = admission.version.data_source();
    let reference = admission.classification_ref.clone();
    let publish = {
        let registered = services.registered.clone();
        let slot = slot.clone();
        let meta = admission.meta.clone();
        let reference = reference.clone();
        move || {
            let classification = slot
                .lock()
                .map_err(|_| "support.internal_error".to_owned())?
                .take()
                .ok_or_else(|| "support.internal_error".to_owned())?;
            registered
                .register(reference.clone(), classification, meta)
                .map_err(|_| "support.internal_error".to_owned())?;
            Ok(reference)
        }
    };
    let body = Arc::new(Body {
        admission,
        registry: services.registry.clone(),
        slot,
    });
    let runtime = &services.assembly.runtime;
    let submitted = runtime
        .submit_with_publication(
            Submission {
                kind_id: JOB_KIND.into(),
                deduplication_key: Some(DeduplicationKey {
                    scope: version_id.as_str().to_owned(),
                    input: key.clone(),
                    channel: "gui".into(),
                    key,
                }),
                body,
                priority: JobPriority::Normal,
                scope_summary: "virtual group classification".into(),
                correlation_id: caller.correlation_id.clone(),
                sample: data_source == DataSource::Sample,
            },
            publish,
        )
        .map_err(runtime_error)?;
    let (job, reference) = match submitted {
        SubmitResult::Created(job) => {
            services.registered.track(job.clone(), reference.clone());
            (job, reference)
        }
        SubmitResult::Existing(job) => {
            let known = services.registered.tracked(&job).ok_or_else(internal)?;
            (job, known)
        }
    };
    services.assembly.workers.notify();
    services.channels.set_job(view, seq, job.clone())?;
    Ok(GroupClassifyResponse::Job {
        request_seq: seq,
        job_ref: job,
        classification_ref: reference,
        result_version: version_id,
        data_source: Some(data_source),
    })
}

fn runtime_error(error: RuntimeError) -> EngineError {
    match error {
        RuntimeError::QueueFull | RuntimeError::DeliveryBusy => EngineError::reason("query.busy"),
        RuntimeError::ShuttingDown => EngineError::reason("job.shutting_down"),
        _ => internal(),
    }
}

struct Body {
    admission: Admission,
    registry: Registry,
    slot: Arc<Mutex<Option<Classification>>>,
}

struct JobControl<'a> {
    context: &'a dyn JobContext,
    total: u64,
}

impl Control for JobControl<'_> {
    fn cancelled(&self) -> bool {
        self.context.checkpoint() == Checkpoint::Cancel
    }

    /// Reports evaluated files against the scope's file count (F24 job.progress).
    fn progress(&self, completed_files: u64) {
        self.context.report_progress(JobProgress {
            completed: completed_files,
            total: Some(self.total),
        });
    }
}

impl JobBody for Body {
    fn kind_id(&self) -> &str {
        JOB_KIND
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let control = JobControl {
            context: context.as_ref(),
            total: self.admission.files,
        };
        let failure = match run(&self.admission, &self.registry, &control) {
            Ok(classification) => match self.slot.lock() {
                Ok(mut slot) => {
                    *slot = Some(classification);
                    None
                }
                Err(_) => Some("support.internal_error"),
            },
            Err(group::Error::Cancelled) => None,
            Err(group::Error::Source(QueryError::StaleVersion)) => Some("result.version_stale"),
            Err(_) => Some("support.internal_error"),
        };
        JobOutcome {
            counts: Default::default(),
            completed_scope: failure.is_none() && context.checkpoint() == Checkpoint::Continue,
            failure_reason_key: failure.map(str::to_owned),
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}
