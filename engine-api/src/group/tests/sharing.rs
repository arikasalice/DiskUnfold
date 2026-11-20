//! Views that share, release and lose a deduplicated classification job, and the limits
//! that end a request with an error.
use std::{sync::mpsc, time::Duration};

use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id,
    group_ipc::GroupCancelOutcome,
    job::{JobBody, JobContext, JobOutcome, JobState},
};
use du_job_runtime::{DeduplicationKey, JobPriority, JobRuntime, Submission, SubmitResult};
use serde_json::{Value, json};

use super::{
    Harness, MemoryRules, TestResult, at_least, ends_with, harness, harness_custom, id,
    save_video_and_big,
};
use crate::api::{CallerCtx, Channel};
use crate::group::{JOB_KIND, channels::Channels, descriptor, internal};
use std::sync::Arc;

type Boxed<T> = Result<T, Box<dyn std::error::Error>>;

fn debug<E: std::fmt::Debug>(error: E) -> String {
    format!("{error:?}")
}

/// A view whose classification always becomes a job (see `FORCE_JOB_PREFIX`).
fn job_view(n: u8) -> Id {
    match Id::parse(&format!("018f3333-0000-7000-8000-00000000f0{n:02x}")) {
        Ok(view) => view,
        Err(_) => id(n),
    }
}

/// The saved ruleset of the harness, so that views share one deduplicated job.
fn saved() -> Value {
    json!({ "kind": "saved", "rulesetVersion": 1 })
}

/// A draft names new groups every time, so each request gets its own job.
fn draft(suffix: &str) -> Value {
    json!({ "kind": "draft", "rules": [{ "displayName": "R", "condition": ends_with(suffix) }] })
}

fn classify_in(harness: &Harness, view: &Id, seq: u32, rules: Value) -> Boxed<Value> {
    harness.ok(
        "group.classify",
        json!({ "viewRef": view, "requestSeq": seq, "rules": rules }),
    )
}

fn job_of(response: &Value) -> Boxed<Id> {
    assert_eq!(response["kind"], "job", "{response}");
    Ok(Id::parse(response["jobRef"].as_str().ok_or("jobRef")?)?)
}

fn cancel_view(harness: &Harness, view: &Id, seq: u32) -> Boxed<Value> {
    harness.ok(
        "group.cancel",
        json!({ "kind": "view", "viewRef": view, "requestSeq": seq }),
    )
}

fn idle() -> Boxed<Harness> {
    let harness = harness_custom(
        Arc::new(MemoryRules::default()),
        true,
        du_analysis_index::group::MAX_DECISIONS,
    )?;
    save_video_and_big(&harness)?;
    Ok(harness)
}

#[test]
fn views_sharing_one_job_cancel_it_only_when_the_last_view_leaves() -> TestResult {
    let harness = idle()?;
    let (first, second) = (job_view(1), job_view(2));
    let a = classify_in(&harness, &first, 1, saved())?;
    let b = classify_in(&harness, &second, 1, saved())?;
    let job = job_of(&a)?;
    assert_eq!(job_of(&b)?, job);
    assert_eq!(a["classificationRef"], b["classificationRef"]);
    let one = cancel_view(&harness, &first, 1)?;
    assert_eq!(one["outcome"], "accepted");
    assert_eq!(harness.runtime.state(&job)?, JobState::Queued);
    assert_eq!(cancel_view(&harness, &first, 1)?["outcome"], "no_effect");
    let last = cancel_view(&harness, &second, 1)?;
    assert_eq!(last["outcome"], "accepted");
    assert_eq!(harness.runtime.state(&job)?, JobState::Cancelled);
    Ok(())
}

/// A job that only waits in the runtime; submitting the same key again returns it.
struct Parked;

impl JobBody for Parked {
    fn kind_id(&self) -> &str {
        JOB_KIND
    }

    fn run(&self, _: Arc<dyn JobContext>) -> JobOutcome {
        JobOutcome {
            counts: Default::default(),
            completed_scope: false,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: None,
        }
    }
}

fn parked_runtime() -> Boxed<JobRuntime> {
    let runtime = super::admission::runtime()?;
    runtime.register(descriptor()).map_err(debug)?;
    Ok(runtime)
}

fn submit_parked(runtime: &JobRuntime) -> Result<Id, crate::error::EngineError> {
    let submitted = runtime
        .submit_with_publication(
            Submission {
                kind_id: JOB_KIND.into(),
                deduplication_key: Some(DeduplicationKey {
                    scope: "v".into(),
                    input: "k".into(),
                    channel: "gui".into(),
                    key: "k".into(),
                }),
                body: Arc::new(Parked),
                priority: JobPriority::Normal,
                scope_summary: "parked".into(),
                scope_roots: Vec::new(),
                correlation_id: id(60),
                sample: false,
            },
            || Ok(id(61)),
        )
        .map_err(|_| internal())?;
    Ok(match submitted {
        SubmitResult::Created(job) | SubmitResult::Existing(job) => job,
    })
}

#[test]
fn a_view_that_receives_a_job_during_its_submission_waits_for_the_binding() -> TestResult {
    let runtime = parked_runtime()?;
    let channels = Channels::default();
    let (a, b) = (id(7), id(8));
    for view in [&a, &b] {
        channels.admit(view, 1, &runtime).map_err(debug)?;
    }
    let (entered, inside) = mpsc::channel();
    let job = std::thread::scope(|scope| -> Boxed<Id> {
        let creating = scope.spawn(|| {
            channels.bind_submission((&a, 1), &runtime, || {
                let job = submit_parked(&runtime)?;
                let _ = entered.send(());
                // The window in which another view could receive the job as a duplicate.
                std::thread::sleep(Duration::from_millis(150));
                Ok((job, id(31)))
            })
        });
        inside.recv_timeout(Duration::from_secs(10))?;
        let (job, _) = channels
            .bind_submission((&b, 1), &runtime, || Ok((submit_parked(&runtime)?, id(31))))
            .map_err(debug)?;
        // View a holds the job by now, so view b only releases its own interest.
        let released = channels.cancel(&b, 1, &runtime).map_err(debug)?;
        assert_eq!(released, GroupCancelOutcome::Accepted);
        assert_eq!(runtime.state(&job)?, JobState::Queued);
        creating
            .join()
            .map_err(|_| "creating thread panicked")?
            .map_err(debug)?;
        Ok(job)
    })?;
    let last = channels.cancel(&a, 1, &runtime).map_err(debug)?;
    assert_eq!(last, GroupCancelOutcome::Accepted);
    assert_eq!(runtime.state(&job)?, JobState::Cancelled);
    Ok(())
}

#[test]
fn a_job_bound_after_its_view_moved_on_is_released_unless_another_view_holds_it() -> TestResult {
    let runtime = parked_runtime()?;
    let channels = Channels::default();
    let (a, b) = (id(7), id(8));
    channels.admit(&a, 1, &runtime).map_err(debug)?;
    channels.admit(&a, 2, &runtime).map_err(debug)?;
    // Nobody else holds the job: it is cancelled when its stale request is bound.
    let (job, _) = channels
        .bind_submission((&a, 1), &runtime, || Ok((submit_parked(&runtime)?, id(31))))
        .map_err(debug)?;
    assert_eq!(runtime.state(&job)?, JobState::Cancelled);
    // The latest request of view a is untouched by the stale one.
    let latest = channels.cancel(&a, 2, &runtime).map_err(debug)?;
    assert_eq!(latest, GroupCancelOutcome::Accepted);

    let runtime = parked_runtime()?;
    let channels = Channels::default();
    for view in [&a, &b] {
        channels.admit(view, 1, &runtime).map_err(debug)?;
    }
    channels.admit(&a, 2, &runtime).map_err(debug)?;
    let bound = |view: &Id, seq| {
        channels
            .bind_submission((view, seq), &runtime, || {
                Ok((submit_parked(&runtime)?, id(31)))
            })
            .map(|(job, _)| job)
            .map_err(debug)
    };
    let held = bound(&b, 1)?;
    let stale = bound(&a, 1)?;
    assert_eq!(stale, held);
    assert_eq!(runtime.state(&held)?, JobState::Queued);
    Ok(())
}

#[test]
fn a_finished_job_no_longer_counts_its_views_as_holders() -> TestResult {
    let harness = idle()?;
    let (first, second) = (job_view(1), job_view(2));
    let job = job_of(&classify_in(&harness, &first, 1, saved())?)?;
    classify_in(&harness, &second, 1, saved())?;
    assert!(harness.runtime.run_next().is_ok_and(|ran| ran.is_some()));
    assert_eq!(harness.runtime.state(&job)?, JobState::Done);
    // View a still names the finished job, but there is nothing left to wait for.
    let outcome = cancel_view(&harness, &second, 1)?;
    assert_eq!(outcome["outcome"], "already_terminal");
    Ok(())
}

#[test]
fn dropping_the_least_recently_used_channel_releases_its_job() -> TestResult {
    let harness = idle()?;
    let (first, second) = (job_view(1), job_view(2));
    let shared = job_of(&classify_in(&harness, &first, 1, saved())?)?;
    classify_in(&harness, &second, 1, saved())?;
    let alone = job_of(&classify_in(&harness, &job_view(3), 1, draft(".txt"))?)?;
    // Five more channels fill the eight; each one is a finished synchronous request.
    for n in 10..15u8 {
        classify_in(&harness, &id(n), 1, draft(".mp4"))?;
    }
    assert_eq!(harness.runtime.state(&shared)?, JobState::Queued);
    // The ninth channel drops view 1; view 2 still waits for the shared job.
    classify_in(&harness, &id(15), 1, draft(".mp4"))?;
    assert_eq!(harness.runtime.state(&shared)?, JobState::Queued);
    // Dropping view 2 releases the last interest in the shared job.
    classify_in(&harness, &id(16), 1, draft(".mp4"))?;
    assert_eq!(harness.runtime.state(&shared)?, JobState::Cancelled);
    assert_eq!(harness.runtime.state(&alone)?, JobState::Queued);
    classify_in(&harness, &id(17), 1, draft(".mp4"))?;
    assert_eq!(harness.runtime.state(&alone)?, JobState::Cancelled);
    Ok(())
}

fn two_rules() -> Value {
    json!({ "kind": "draft", "rules": [
        { "displayName": "Video", "condition": ends_with(".mp4") },
        { "displayName": "Big", "condition": at_least(10) },
    ] })
}

#[test]
fn a_scope_beyond_the_decision_limit_is_refused_and_leaves_no_work_behind() -> TestResult {
    // Four files and two rules are eight decisions.
    let harness = harness_custom(Arc::new(MemoryRules::default()), true, 7)?;
    let view = id(7);
    let (_, error) = harness.call(
        "group.classify",
        json!({ "viewRef": view, "requestSeq": 1, "rules": two_rules() }),
    )?;
    assert_eq!(error.as_deref(), Some("group.rule_limit"));
    // The failed request released its channel entry, so a late cancel has nothing to stop.
    assert_eq!(cancel_view(&harness, &view, 1)?["outcome"], "no_effect");

    let job = job_of(&classify_in(&harness, &job_view(1), 1, two_rules())?)?;
    assert!(harness.runtime.run_next().is_ok_and(|ran| ran.is_some()));
    let snapshot = harness.runtime.snapshot(&job)?;
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(
        snapshot.failure_reason_key.as_deref(),
        Some("group.rule_limit")
    );

    let roomy = harness_custom(Arc::new(MemoryRules::default()), true, 8)?;
    let summary = classify_in(&roomy, &view, 1, two_rules())?;
    assert_eq!(summary["kind"], "summary");
    Ok(())
}

#[test]
fn cancel_without_running_work_has_no_effect_and_other_channels_are_refused() -> TestResult {
    let harness = harness()?;
    let cancelled = harness.ok(
        "group.cancel",
        json!({ "kind": "view", "viewRef": id(7), "requestSeq": 1 }),
    )?;
    assert_eq!(cancelled["outcome"], "no_effect");
    let correlation = Id::parse("018f2222-0000-7000-8000-000000000001")?;
    let result = harness.engine.call(
        CallEnvelope {
            name: "group.rules.get".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: correlation.clone(),
            body: json!({}),
        },
        &CallerCtx::new(Channel::Mcp, correlation, None),
    );
    assert_eq!(
        result.error.map(|error| error.reason_key).as_deref(),
        Some("command.not_provided")
    );
    Ok(())
}
