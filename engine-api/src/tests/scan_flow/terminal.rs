use super::*;

pub(super) fn check_terminal_scan(
    engine: &Engine,
    summary: JobSummary,
    history_recorded: bool,
    job_ref: &Id,
) -> Result<Id, Box<dyn std::error::Error>> {
    assert_eq!(&summary.job_id, job_ref);
    assert_eq!(summary.scope_summary, "metadata: 1 root");
    assert!(history_recorded);
    let version = summary.result_version_id.ok_or("missing result version")?;
    let status = call(
        engine,
        "job.status",
        Channel::Gui,
        ENVELOPE_VERSION,
        json!({"jobRef": job_ref}),
    )?;
    let recovered = &status.body["terminal"];
    assert_eq!(recovered["state"], status.body["state"]);
    assert_eq!(recovered["sequence"], status.body["sequence"]);
    assert_eq!(recovered["completed"], summary.counts.completed);
    assert_eq!(recovered["historyRecorded"], true);
    assert_eq!(recovered["resultVersion"], json!(version));
    assert!(recovered.get("warning").is_none());
    check_terminal_list(engine, job_ref, recovered)?;
    Ok(version)
}

fn check_terminal_list(
    engine: &Engine,
    job_ref: &Id,
    terminal: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let list = |body| call(engine, "job.list", Channel::Gui, ENVELOPE_VERSION, body);
    let finished = list(json!({"states": ["done"]}))?;
    let row = finished
        .body
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["jobRef"] == job_ref.as_str()))
        .ok_or("missing scan in done jobs")?;
    assert_eq!(row["terminal"], *terminal);
    assert_eq!(row["scopeSummary"], "metadata: 1 root");
    assert_eq!(row["kind"], "scan.metadata");
    assert_eq!(row["waitReason"], "none");
    assert_eq!(row["priority"], "normal");
    assert_eq!(row["sequence"], terminal["sequence"]);
    assert!(
        row["supportedControls"]
            .as_array()
            .is_some_and(|controls| { controls.iter().any(|control| control == "cancel") })
    );
    assert_eq!(list(json!({"states": []}))?.body, json!([]));
    assert_eq!(
        list(json!({"states": ["unknown"]}))?
            .error
            .ok_or("invalid state accepted")?
            .reason_key,
        "command.body_invalid",
    );
    Ok(())
}
