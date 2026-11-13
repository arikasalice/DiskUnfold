use du_core_contracts::visualization::render::{VizColorMode, VizFallback, VizLabelMode};
use serde_json::{from_value, json, to_value};

#[test]
fn fallback_requires_nonempty_model_and_revision() -> Result<(), Box<dyn std::error::Error>> {
    let valid = json!({"modelRef":"m1","resultRevision":"018f0000-0000-7000-8000-000000000001"});
    let result: VizFallback = from_value(valid.clone())?;
    assert_eq!(to_value(result)?, valid);
    for bad in [
        json!({"modelRef":"", "resultRevision":"018f0000-0000-7000-8000-000000000001"}),
        json!({"modelRef":"m"}),
        json!({"modelRef":"m", "resultRevision":null}),
        json!({"modelRef":"m", "resultRevision":"018f0000-0000-7000-8000-000000000001", "extra": 1}),
    ] {
        assert!(from_value::<VizFallback>(bad).is_err());
    }
    Ok(())
}

#[test]
fn tree_options_preserve_exact_choices() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(to_value(VizColorMode::ParentFolder)?, "parent_folder");
    assert_eq!(to_value(VizColorMode::Classification)?, "classification");
    assert_eq!(to_value(VizColorMode::Monochrome)?, "monochrome");
    assert_eq!(to_value(VizLabelMode::NameSize)?, "name_size");
    assert_eq!(to_value(VizLabelMode::NameOnly)?, "name_only");
    assert!(from_value::<VizColorMode>(json!("future")).is_err());
    Ok(())
}
