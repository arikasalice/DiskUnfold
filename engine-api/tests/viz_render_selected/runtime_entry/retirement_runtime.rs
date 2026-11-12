use super::*;

#[test]
fn last_detach_and_session_retirement_cancel_without_cache_publication() -> Result<(), String> {
    let fixture = Fixture::new([25, 75])?;
    let a = fixture.open()?;
    let first = job(&fixture.render(&a, 1)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    assert_eq!(fixture.detach(&a, 1)?["controlResult"], "accepted");
    assert_eq!(
        fixture.jobs.state(&first).map_err(|e| e.to_string())?,
        JobState::CancelRequested
    );
    fixture.engine.retire_window_session();
    fixture.release.send(()).map_err(|e| e.to_string())?;
    fixture.finished(&first, JobState::Cancelled)?;
    assert_eq!(
        fixture
            .call("viz.render", render_body(&a, 2))
            .error
            .ok_or("retired channel accepted")?
            .reason_key,
        "ref.unavailable"
    );
    Ok(())
}
