use super::*;

#[test]
fn the_classification_criterion_is_a_required_request_field() -> TestResult {
    let harness = Harness::product(2000)?;
    harness.publish(&two_files(3))?;
    let view = harness.open_view()?;
    let mut body = render_body(&view, 1, "treemap", 3);
    body.as_object_mut()
        .ok_or("not an object")?
        .remove("classificationCriterion");
    let error = harness
        .call("viz.render", body)
        .error
        .ok_or("a render without a classification criterion succeeded")?;
    assert_eq!(error.reason_key, "command.body_invalid");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    Ok(())
}
