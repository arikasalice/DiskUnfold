//! The transition table: every state against every command.

use du_core_contracts::{
    Id,
    job::JobState,
    sample_ipc::{
        SAMPLE_BUNDLE_INVALID, SAMPLE_FORMAT_UNSUPPORTED, SAMPLE_LOAD_FAILED, SAMPLE_MISSING,
        SAMPLE_REAL_ANALYSIS_ACTION, SampleEventState as E, SampleSession, SampleSessionState as S,
    },
};
use du_job_runtime::{JobPriority, RuntimeError};

use super::{Rig, closed_error, ok};
use crate::sample::KIND;

fn job(view: &SampleSession) -> Result<Id, String> {
    view.job_ref.clone().ok_or_else(|| "no job".to_owned())
}

fn states(rig: &Rig) -> Vec<(E, E)> {
    rig.events()
        .iter()
        .map(|event| (event.state, event.previous_state))
        .collect()
}

fn active(rig: &Rig) -> Result<SampleSession, String> {
    rig.seed_observations(2);
    let opened = ok(rig.open())?;
    rig.finish(&job(&opened)?, JobState::Done);
    ok(rig.status(&opened.session_ref))
}

fn unavailable(rig: &Rig) -> Result<SampleSession, String> {
    rig.fail_bundle(SAMPLE_BUNDLE_INVALID);
    ok(rig.open())
}

fn closed(rig: &Rig) -> Result<Id, String> {
    let opened = ok(rig.open())?;
    ok(rig.close(&opened.session_ref))?;
    Ok(opened.session_ref)
}

#[test]
fn open_submits_one_sample_job_and_is_idempotent() -> Result<(), String> {
    let rig = Rig::new();
    let first = ok(rig.open())?;
    assert_eq!((first.state, first.sequence), (S::Preparing, 1));
    assert_eq!(first.bundle_version.as_deref(), Some("b1"));
    assert!(first.observations.is_empty() && first.plans.is_empty());
    assert_eq!(ok(rig.open())?, first);
    assert_eq!(rig.submissions(), 1);
    let submitted = rig.jobs.submitted.lock().map_err(|e| e.to_string())?;
    let recorded = submitted.first().ok_or("no submission")?;
    assert_eq!(recorded.kind_id, KIND);
    assert!(recorded.sample && recorded.priority == JobPriority::Normal);
    assert_eq!(recorded.scope_summary, "sample");
    let key = recorded.dedup.as_ref().ok_or("no key")?;
    assert_eq!(key.scope, first.session_ref.as_str());
    assert_eq!(key.input, "b1");
    drop(submitted);
    assert_eq!(states(&rig), [(E::Loading, E::Inactive)]);
    Ok(())
}

#[test]
fn invalid_bundles_are_unavailable_without_any_job() -> Result<(), String> {
    for reason in [
        SAMPLE_MISSING,
        SAMPLE_FORMAT_UNSUPPORTED,
        SAMPLE_BUNDLE_INVALID,
    ] {
        let rig = Rig::new();
        rig.fail_bundle(reason);
        let view = ok(rig.open())?;
        assert_eq!(view.state, S::Unavailable);
        assert_eq!(view.unavailable_reason.as_deref(), Some(reason));
        assert_eq!(
            view.real_analysis_action.as_deref(),
            Some(SAMPLE_REAL_ANALYSIS_ACTION)
        );
        assert!(view.job_ref.is_none() && view.bundle_version.is_none());
        assert_eq!((rig.submissions(), rig.cancelled().len()), (0, 0));
        assert_eq!(
            rig.loader.built.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        let events = rig.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state, E::Failed);
        assert_eq!(events[0].reason_key.as_deref(), Some(reason));
        assert_eq!(ok(rig.open())?, view);
    }
    Ok(())
}

#[test]
fn a_full_queue_leaves_no_session_and_reports_busy() -> Result<(), String> {
    let rig = Rig::new();
    *rig.jobs.fail.lock().map_err(|e| e.to_string())? = Some(RuntimeError::QueueFull);
    assert_eq!(
        rig.open(),
        Err(crate::error::EngineError::reason("job.queue_full"))
    );
    assert!(rig.events().is_empty());
    assert_eq!(rig.status(&super::job_id(1)), Err(closed_error()));
    *rig.jobs.fail.lock().map_err(|e| e.to_string())? = None;
    let view = ok(rig.open())?;
    assert_eq!((view.state, view.sequence), (S::Preparing, 1));
    Ok(())
}

#[test]
fn done_activates_and_records_observations_in_order() -> Result<(), String> {
    let rig = Rig::new();
    let view = active(&rig)?;
    assert_eq!((view.state, view.sequence), (S::Active, 2));
    let indexes: Vec<u32> = view.observations.iter().map(|o| o.index).collect();
    assert_eq!(indexes, [0, 1]);
    assert!(view.job_ref.is_none() && view.real_analysis_action.is_none());
    assert_eq!(
        states(&rig),
        [(E::Loading, E::Inactive), (E::Active, E::Loading)]
    );
    Ok(())
}

#[test]
fn partial_or_failed_loads_become_unavailable_and_release_the_versions() -> Result<(), String> {
    for end in [JobState::Partial, JobState::Failed] {
        let rig = Rig::new();
        let opened = ok(rig.open())?;
        rig.finish(&job(&opened)?, end);
        let view = ok(rig.status(&opened.session_ref))?;
        assert_eq!((view.state, view.sequence), (S::Unavailable, 2));
        assert_eq!(view.unavailable_reason.as_deref(), Some(SAMPLE_LOAD_FAILED));
        assert_eq!(rig.releases(), 1);
        assert!(view.observations.is_empty());
        let last = rig.events().pop().ok_or("no event")?;
        assert_eq!(last.reason_key.as_deref(), Some(SAMPLE_LOAD_FAILED));
        assert_eq!(last.job_ref, opened.job_ref);
    }
    Ok(())
}

#[test]
fn unrelated_job_ends_change_nothing() -> Result<(), String> {
    let rig = Rig::new();
    let opened = ok(rig.open())?;
    let job_ref = job(&opened)?;
    rig.finish(&job_ref, JobState::Cancelled);
    rig.finish(&super::job_id(77), JobState::Done);
    assert_eq!(ok(rig.status(&opened.session_ref))?, opened);
    rig.finish(&job_ref, JobState::Done);
    let done = ok(rig.status(&opened.session_ref))?;
    assert_eq!(done.state, S::Active);
    // A repeated end, and an end after close, are ignored.
    rig.finish(&job_ref, JobState::Failed);
    assert_eq!(ok(rig.status(&opened.session_ref))?, done);
    ok(rig.close(&opened.session_ref))?;
    rig.finish(&job_ref, JobState::Done);
    assert_eq!(rig.events().len(), 3);
    Ok(())
}

#[test]
fn reset_while_loading_answers_with_the_same_job_and_submits_nothing() -> Result<(), String> {
    let rig = Rig::new();
    let opened = ok(rig.open())?;
    for _ in 0..3 {
        assert_eq!(ok(rig.reset(&opened.session_ref))?, opened);
    }
    assert_eq!(
        (rig.submissions(), rig.releases(), rig.events().len()),
        (1, 0, 1)
    );
    Ok(())
}

#[test]
fn reset_reloads_an_active_session() -> Result<(), String> {
    let rig = Rig::new();
    let before = active(&rig)?;
    let view = ok(rig.reset(&before.session_ref))?;
    assert_eq!((view.state, view.sequence), (S::Preparing, 4));
    assert!(view.observations.is_empty());
    assert_ne!(view.job_ref, None);
    assert_eq!(rig.submissions(), 2);
    assert_eq!(rig.releases(), 1);
    assert_eq!(
        states(&rig)[2..],
        [(E::Loading, E::Active), (E::Loading, E::Loading)]
    );
    Ok(())
}

#[test]
fn reset_revalidates_an_unavailable_session() -> Result<(), String> {
    let rig = Rig::new();
    let before = unavailable(&rig)?;
    let again = ok(rig.reset(&before.session_ref))?;
    assert_eq!((again.state, again.sequence), (S::Unavailable, 3));
    assert_eq!(rig.submissions(), 0);
    rig.fix_bundle();
    let view = ok(rig.reset(&before.session_ref))?;
    assert_eq!((view.state, view.sequence), (S::Preparing, 5));
    assert!(view.unavailable_reason.is_none() && view.real_analysis_action.is_none());
    assert_eq!(rig.submissions(), 1);
    Ok(())
}

#[test]
fn reset_that_cannot_queue_becomes_unavailable() -> Result<(), String> {
    let rig = Rig::new();
    let before = active(&rig)?;
    *rig.jobs.fail.lock().map_err(|e| e.to_string())? = Some(RuntimeError::QueueFull);
    let view = ok(rig.reset(&before.session_ref))?;
    assert_eq!(view.state, S::Unavailable);
    assert_eq!(view.unavailable_reason.as_deref(), Some(SAMPLE_LOAD_FAILED));
    Ok(())
}

#[test]
fn close_cancels_the_job_before_releasing_versions() -> Result<(), String> {
    let rig = Rig::new();
    let opened = ok(rig.open())?;
    let result = ok(rig.close(&opened.session_ref))?;
    assert_eq!((result.state, result.released_versions), (S::Closed, 3));
    assert_eq!(rig.cancelled(), [job(&opened)?]);
    assert_eq!(
        *rig.log.lock().map_err(|e| e.to_string())?,
        ["cancel", "release"]
    );
    let last = rig.events().pop().ok_or("no event")?;
    assert_eq!(
        (last.state, last.previous_state, last.sequence),
        (E::Inactive, E::Loading, 2)
    );
    // Closing again repeats the result without a job cancel, release or event.
    assert_eq!(ok(rig.close(&opened.session_ref))?, result);
    assert_eq!(
        (rig.cancelled().len(), rig.releases(), rig.events().len()),
        (1, 1, 2)
    );
    Ok(())
}

#[test]
fn close_from_active_and_unavailable_cancels_nothing() -> Result<(), String> {
    let rig = Rig::new();
    let view = active(&rig)?;
    assert_eq!(ok(rig.close(&view.session_ref))?.released_versions, 3);
    let rig = Rig::new();
    let view = unavailable(&rig)?;
    assert_eq!(ok(rig.close(&view.session_ref))?.state, S::Closed);
    assert!(rig.cancelled().is_empty());
    Ok(())
}

#[test]
fn closed_and_unknown_references_are_not_found() -> Result<(), String> {
    let rig = Rig::new();
    let unknown = super::job_id(4242);
    assert_eq!(rig.status(&unknown), Err(closed_error()));
    assert_eq!(rig.reset(&unknown), Err(closed_error()));
    assert_eq!(rig.close(&unknown), Err(closed_error()));
    let ended = closed(&rig)?;
    assert_eq!(rig.status(&ended), Err(closed_error()));
    assert_eq!(rig.reset(&ended), Err(closed_error()));
    assert_eq!(rig.status(&unknown), Err(closed_error()));
    assert_eq!(rig.submissions(), 1);
    // A new open after close starts a new session; the old reference is gone.
    let fresh = ok(rig.open())?;
    assert_ne!(fresh.session_ref, ended);
    assert_eq!(fresh.sequence, 1);
    assert_eq!(rig.status(&ended), Err(closed_error()));
    assert_eq!(rig.close(&ended), Err(closed_error()));
    Ok(())
}

#[test]
fn a_job_end_delivered_inside_a_command_is_applied_after_it() -> Result<(), String> {
    let rig = Rig::new();
    *rig.jobs
        .finish_on_submit
        .lock()
        .map_err(|e| e.to_string())? = Some(JobState::Done);
    let opened = ok(rig.open())?;
    assert_eq!(opened.state, S::Preparing);
    assert_eq!(ok(rig.status(&opened.session_ref))?.state, S::Active);

    let rig = Rig::new();
    *rig.jobs
        .finish_on_cancel
        .lock()
        .map_err(|e| e.to_string())? = Some(JobState::Cancelled);
    let opened = ok(rig.open())?;
    assert_eq!(ok(rig.close(&opened.session_ref))?.state, S::Closed);
    Ok(())
}

type Trace = Vec<(S, u64)>;
type Wire = Vec<(E, E, Option<String>, u64)>;

/// Plays one fixed command order and records each response and the published events.
fn script() -> Result<(Trace, Wire), String> {
    let rig = Rig::new();
    rig.seed_observations(2);
    let mut trace = Trace::new();
    let mut keep = |view: SampleSession| {
        trace.push((view.state, view.sequence));
        // The response carries the sequence of the newest event.
        assert_eq!(rig.events().last().map(|e| e.sequence), Some(view.sequence));
        view
    };
    let opened = keep(ok(rig.open())?);
    let session = opened.session_ref.clone();
    keep(ok(rig.reset(&session))?);
    rig.finish(&job(&opened)?, JobState::Done);
    let active = keep(ok(rig.status(&session))?);
    let reloading = keep(ok(rig.reset(&session))?);
    rig.finish(&job(&reloading)?, JobState::Failed);
    keep(ok(rig.status(&session))?);
    rig.fix_bundle();
    keep(ok(rig.reset(&session))?);
    let closed = ok(rig.close(&session))?;
    trace.push((closed.state, rig.events().len() as u64));
    ok(rig.close(&session))?;
    assert_ne!(active.job_ref, reloading.job_ref);
    let wire = rig
        .events()
        .into_iter()
        .map(|e| (e.state, e.previous_state, e.reason_key, e.sequence))
        .collect();
    Ok((trace, wire))
}

#[test]
fn events_are_contiguous_and_replaying_the_script_is_deterministic() -> Result<(), String> {
    let (trace, wire) = script()?;
    let sequences: Vec<u64> = wire.iter().map(|event| event.3).collect();
    assert_eq!(sequences, (1..=sequences.len() as u64).collect::<Vec<_>>());
    assert_eq!(script()?, (trace, wire));
    Ok(())
}
