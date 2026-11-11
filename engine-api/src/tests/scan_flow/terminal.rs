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
    Ok(version)
}
