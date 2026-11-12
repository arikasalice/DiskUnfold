use super::*;

#[test]
fn opt_in_required_data_uses_pinned_facts_and_same_revision_model_rows() -> Result<(), String> {
    let fixture = Fixture::with_wait([25, 75], Duration::from_secs(2))?;
    fixture
        .producer
        .required_data_opt_in
        .store(true, Ordering::Relaxed);
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let view = fixture.open()?;
    let first = fixture.render(&view, 1)?;
    let model = if first["kind"] == "job" {
        let physical = job(&first)?;
        fixture.finished(&physical, JobState::Done)?;
        fixture.render(&view, 2)?
    } else {
        first
    };
    assert_eq!(model["kind"], "model");
    let model = &model["model"];
    let rows = fixture.ok(
        "viz.rows",
        json!({
            "modelRef": model["modelRef"],
            "resultRevision": id(3),
            "start": 0,
            "count": 10
        }),
    )?;
    assert_eq!(rows["resultRevision"], json!(id(3)));
    assert_eq!(rows["totalCount"], 2);
    Ok(())
}
