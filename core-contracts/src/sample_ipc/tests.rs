use super::*;

fn id(value: &str) -> Result<Id, Box<dyn std::error::Error>> {
    Ok(Id::parse(value)?)
}

#[test]
fn internal_states_map_to_the_four_published_event_values() {
    use SampleSessionState::*;
    let mapped: Vec<_> = [Preparing, Active, Resetting, Unavailable, Closed]
        .into_iter()
        .map(SampleSessionState::event_state)
        .collect();
    assert_eq!(
        mapped,
        [
            SampleEventState::Loading,
            SampleEventState::Active,
            SampleEventState::Loading,
            SampleEventState::Failed,
            SampleEventState::Inactive,
        ]
    );
}

#[test]
fn open_accepts_only_an_empty_object() {
    assert!(serde_json::from_str::<SampleOpenRequest>("{}").is_ok());
    assert!(serde_json::from_str::<SampleOpenRequest>("{\"scope\":1}").is_err());
    assert!(serde_json::from_str::<SampleOpenRequest>("null").is_err());
}

#[test]
fn session_requests_reject_unknown_fields() -> Result<(), Box<dyn std::error::Error>> {
    let body = r#"{"sessionRef":"0195f856-ab83-7a59-9343-10d29775c9a9"}"#;
    let request: SampleSessionRequest = serde_json::from_str(body)?;
    assert_eq!(
        request.session_ref,
        id("0195f856-ab83-7a59-9343-10d29775c9a9")?
    );
    let extra = r#"{"sessionRef":"0195f856-ab83-7a59-9343-10d29775c9a9","draft":{}}"#;
    assert!(serde_json::from_str::<SampleSessionRequest>(extra).is_err());
    Ok(())
}

#[test]
fn unavailable_session_round_trips_with_reason_and_action() -> Result<(), Box<dyn std::error::Error>>
{
    let session = SampleSession {
        session_ref: id("0195f856-ab83-7a59-9343-10d29775c9a9")?,
        state: SampleSessionState::Unavailable,
        sequence: 1,
        bundle_version: None,
        job_ref: None,
        observations: Vec::new(),
        plans: Vec::new(),
        unavailable_reason: Some(SAMPLE_BUNDLE_INVALID.into()),
        real_analysis_action: Some(SAMPLE_REAL_ANALYSIS_ACTION.into()),
    };
    let wire = serde_json::to_value(&session)?;
    assert_eq!(wire["state"], "unavailable");
    assert_eq!(wire["unavailableReason"], "sample.bundle_invalid");
    assert_eq!(wire["realAnalysisAction"], "sample.start_real_analysis");
    assert!(wire.get("jobRef").is_none());
    assert_eq!(serde_json::from_value::<SampleSession>(wire)?, session);
    Ok(())
}

#[test]
fn state_event_body_uses_published_names() -> Result<(), Box<dyn std::error::Error>> {
    let body = SampleStateChanged {
        session_ref: id("0195f856-ab83-7a59-9343-10d29775c9a9")?,
        state: SampleEventState::Active,
        previous_state: SampleEventState::Loading,
        reason_key: None,
        job_ref: Some(id("0195f856-ab83-7a59-9343-10d29775c9aa")?),
        sequence: 2,
        observed_at: Timestamp::parse("2026-11-14T08:00:00.000Z")?,
    };
    let wire = serde_json::to_value(&body)?;
    assert_eq!(wire["state"], "active");
    assert_eq!(wire["previousState"], "loading");
    assert_eq!(wire["sequence"], 2);
    assert!(wire.get("reasonKey").is_none());
    Ok(())
}
