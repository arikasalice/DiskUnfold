use super::*;

#[test]
fn fast_physical_completion_returns_model_within_configured_wait() -> Result<(), String> {
    let fixture = Fixture::with_wait([25, 75], Duration::from_secs(2))?;
    let a = fixture.open()?;
    fixture.release.send(()).map_err(|e| e.to_string())?;
    let model = fixture.render(&a, 1)?;
    assert_eq!(model["kind"], "model");
    assert_eq!(model["model"]["dataSource"], "real");
    assert_eq!(
        model["model"]["representation"]["colorMode"],
        "parent_folder"
    );
    assert_eq!(
        model["model"]["totals"]["scope"]["quantity"]["value"],
        "100"
    );
    Ok(())
}
