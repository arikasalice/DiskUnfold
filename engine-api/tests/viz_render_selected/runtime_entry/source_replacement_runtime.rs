use super::*;

#[test]
fn source_replacement_beats_commit_for_the_original_admitted_revision() -> Result<(), String> {
    let fixture = Fixture::new([25, 75])?;
    let a = fixture.open()?;
    let first = job(&fixture.render(&a, 1)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    publish(&fixture.sink, id(2), id(4))?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    fixture.finished(&first, JobState::Failed)?;
    assert_eq!(
        fixture
            .call("viz.render", render_body(&a, 2))
            .error
            .ok_or("stale version accepted")?
            .reason_key,
        "result.version_stale"
    );
    Ok(())
}
