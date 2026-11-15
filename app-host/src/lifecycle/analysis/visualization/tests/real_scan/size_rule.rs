//! The stored size rule setting reaches a real scan through the production assembly
//! (F03 FR14.HD2.app-host.DD4).
use serde_json::{Value, json};

use super::super::assemble_with_stored_rule;
use super::{Assembled, Failure, flat_folder, render_model, scan};

/// The size rule the rendered model reports for the scan result it was drawn from.
fn model_rule(assembled: &Assembled) -> Result<Value, Failure> {
    let folder = flat_folder()?;
    let revision = scan(assembled, folder.path())?;
    let model = render_model(assembled, &revision, "treemap")?;
    Ok(model["tableRevisions"]["sizeRule"].clone())
}

#[test]
fn a_stored_registered_rule_id_reaches_the_pinned_scan_result() -> Result<(), Failure> {
    // The production bundle currently has one rule, so this confirms its ID reaches
    // the result through StartupSettings and register_desktop; a second rule is needed
    // to distinguish selection from an unconditional default.
    let assembled = assemble_with_stored_rule("size.agg.subtree-unique")?;
    assert_eq!(
        model_rule(&assembled)?,
        json!({"id": "size.agg.subtree-unique", "revision": 1})
    );
    Ok(())
}

#[test]
fn a_stored_unregistered_rule_id_falls_back_to_the_default_without_failing_the_scan()
-> Result<(), Failure> {
    // A downgraded profile can name a rule this build does not register.
    let assembled = assemble_with_stored_rule("size.agg.removed")?;
    assert_eq!(
        model_rule(&assembled)?,
        json!({"id": "size.agg.subtree-unique", "revision": 1})
    );
    Ok(())
}
