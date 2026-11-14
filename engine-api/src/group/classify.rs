//! `group.classify`, `group.summary` and `group.cancel`.
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use du_analysis_index::{
    QueryError, ResultVersion,
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

use super::{
    JOB_KIND, Meta, Services, codec, hex, internal, pin::pin, summary::summary_response, tables,
};
use crate::{
    api::{CallerCtx, Channel, UseCase},
    browse::folder::lower_condition,
    error::EngineError,
};

/// Provisional synchronous wait (F36 DLD Q3) before work moves to a job.
const SYNC_LIMIT: Duration = Duration::from_millis(500);
/// Results with more items than this go straight to a job. The item count bounds the
/// scope's files without walking the scope at admission.
const SYNC_ITEMS: usize = 200_000;

/// Test-only views that always take the job path: this one and every view whose reference
/// starts with [`FORCE_JOB_PREFIX`], so that several views can share one job.
#[cfg(test)]
pub(super) const FORCE_JOB_VIEW: &str = "018f3333-0000-7000-8000-00000000f00b";
#[cfg(test)]
pub(super) const FORCE_JOB_PREFIX: &str = "018f3333-0000-7000-8000-00000000f0";

fn sync_allowed(view: &Id, items: usize) -> bool {
    #[cfg(test)]
    if view.as_str().starts_with(FORCE_JOB_PREFIX) {
        return false;
    }
    let _ = view;
    items <= SYNC_ITEMS
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
        let version = pin(
            &services.assembly.store,
            request.result_version.as_ref(),
            caller.channel,
        )?;
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
        let admission = Admission {
            version,
            scope: request.scope,
            conditions: resolved.conditions,
            meta,
            classification_ref: services.assembly.ids.next_id(),
        };
        let response = admitted(services, caller, admission, (&view, seq), &flag);
        // A job keeps the channel handle until it ends; every other outcome releases it.
        if !matches!(response, Ok(GroupClassifyResponse::Job { .. })) {
            services.channels.finish(&view, seq);
        }
        response
    }
}

/// Runs an admitted request: a registered duplicate, a synchronous run or a job.
fn admitted(
    services: &Services,
    caller: &CallerCtx,
    admission: Admission,
    channel: (&Id, u32),
    flag: &AtomicBool,
) -> Result<GroupClassifyResponse, EngineError> {
    let (view, seq) = channel;
    let store = &services.assembly.store;
    if let Some(found) = services.registered.find(&admission.meta.key, store) {
        return summary_response(services, seq, &found);
    }
    if sync_allowed(view, admission.version.items().len()) {
        let control = SyncControl {
            caller,
            flag,
            deadline: Instant::now() + SYNC_LIMIT,
            expired: AtomicBool::new(false),
        };
        match run(
            &admission,
            &services.registry,
            services.assembly.decision_limit,
            &control,
        ) {
            Ok(classification) => {
                services.registered.register(
                    admission.classification_ref.clone(),
                    classification,
                    admission.meta.clone(),
                )?;
                return summary_response(services, seq, &admission.classification_ref);
            }
            Err(group::Error::Cancelled) if control.timed_out() => {}
            Err(error) => return Err(classify_error(error)),
        }
    }
    submit(services, caller, admission, channel)
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
}

fn run(
    admission: &Admission,
    registry: &Registry,
    limit: usize,
    control: &dyn Control,
) -> Result<Classification, group::Error> {
    group::classify_scope_bounded(
        admission.version.clone(),
        admission.scope.as_ref(),
        registry,
        admission.conditions.clone(),
        Parallelism::default(),
        control,
        limit,
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
            let mut lowered = Vec::with_capacity(drafts.len());
            for draft in drafts {
                lowered.push(ruleset::RuleDraft {
                    group_id: draft.group_id,
                    display_name: draft.display_name,
                    condition: lower_condition(draft.condition)
                        .map_err(|_| EngineError::reason("query.syntax_invalid"))?,
                });
            }
            // Drafts pass the checks of a save; group IDs must name saved rules.
            let saved = codec::load(services.rules.as_ref())?;
            let limits = ruleset::RuleLimits::default();
            let issues = ruleset::review(&saved.ruleset, &lowered, &services.registry, limits);
            if let Some(issue) = issues.first() {
                return Err(EngineError::reason(issue.reason_key));
            }
            let rules: Vec<ruleset::Rule> = lowered
                .into_iter()
                .map(|draft| {
                    let condition = draft.condition.normalize();
                    ruleset::Rule {
                        group_id: draft
                            .group_id
                            .unwrap_or_else(|| services.assembly.ids.next_id()),
                        display_name: ruleset::normalize_name(&draft.display_name),
                        predicates: ruleset::strategies(&condition),
                        condition,
                    }
                })
                .collect();
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
    // Duplicate key: result version, scope, ruleset version, rule content and group
    // identities. The ruleset version keeps a summary from reporting an older version.
    let key = format!(
        "{}|{}|{}|{}|{}",
        version.version_id().as_str(),
        scope.map_or_else(|| "*".to_owned(), |item| item.item_no.to_string()),
        ruleset_version,
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

fn classify_error(error: group::Error) -> EngineError {
    match error {
        group::Error::InvalidRoot => EngineError::reason("cursor.invalid"),
        group::Error::Source(QueryError::StaleVersion) => {
            EngineError::reason("result.version_stale")
        }
        group::Error::Source(QueryError::NotFound) => EngineError::reason("ref.unavailable"),
        group::Error::InvalidRule(_) => EngineError::reason("query.syntax_invalid"),
        group::Error::ScopeTooLarge => EngineError::reason("group.rule_limit"),
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
        decision_limit: services.assembly.decision_limit,
        slot,
    });
    let runtime = &services.assembly.runtime;
    // Submission, reference tracking and the channel binding happen under the channel
    // lock, so a view that receives this job as a duplicate finds it tracked and counts
    // this view as a holder.
    let (job, reference) = services
        .channels
        .bind_submission((view, seq), runtime, || {
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
            match submitted {
                SubmitResult::Created(job) => {
                    services.registered.track(job.clone(), reference.clone());
                    Ok((job, reference))
                }
                SubmitResult::Existing(job) => {
                    let known = services.registered.tracked(&job).ok_or_else(internal)?;
                    Ok((job, known))
                }
            }
        })?;
    services.assembly.workers.notify();
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
    decision_limit: usize,
    slot: Arc<Mutex<Option<Classification>>>,
}

struct JobControl<'a> {
    context: &'a dyn JobContext,
    /// Files in the scope, or [`UNKNOWN_TOTAL`] until the scope has been collected.
    total: AtomicU64,
}

const UNKNOWN_TOTAL: u64 = u64::MAX;

impl Control for JobControl<'_> {
    fn cancelled(&self) -> bool {
        self.context.checkpoint() == Checkpoint::Cancel
    }

    /// Reports evaluated files against the scope's file count (F24 job.progress).
    fn progress(&self, completed_files: u64) {
        let total = self.total.load(Ordering::Acquire);
        self.context.report_progress(JobProgress {
            completed: completed_files,
            total: (total != UNKNOWN_TOTAL).then_some(total),
        });
    }

    fn scope_files(&self, files: u64) {
        self.total.store(files, Ordering::Release);
    }
}

impl JobBody for Body {
    fn kind_id(&self) -> &str {
        JOB_KIND
    }

    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let control = JobControl {
            context: context.as_ref(),
            total: AtomicU64::new(UNKNOWN_TOTAL),
        };
        let failure = match run(
            &self.admission,
            &self.registry,
            self.decision_limit,
            &control,
        ) {
            Ok(classification) => match self.slot.lock() {
                Ok(mut slot) => {
                    *slot = Some(classification);
                    None
                }
                Err(_) => Some("support.internal_error"),
            },
            Err(group::Error::Cancelled) => None,
            Err(group::Error::Source(QueryError::StaleVersion)) => Some("result.version_stale"),
            Err(group::Error::ScopeTooLarge) => Some("group.rule_limit"),
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
