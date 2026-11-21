use super::*;
use du_core_contracts::Id;
use du_engine_api::api::Channel;
use serde_json::json;

fn caller() -> CallerCtx {
    CallerCtx::new(Channel::Cli, Id::from_v7_bytes([7; 16]), None)
}

#[test]
fn a_review_keeps_its_fingerprint_and_drops_every_path() -> Result<(), String> {
    let review = json!({
        "scope": {"includes": [{"display": "C:\\Secret"}]},
        "notices": ["scope.exclusion_ineffective", "C:\\Secret"],
        "targets": [
            {"path": {"display": "C:\\Secret"}, "access": "accessible", "reasonKey": null},
            "C:\\Secret"
        ],
        "reviewFingerprint": "abc",
        "observedAt": "2026-11-20T00:00:00Z",
        "requestSeq": 1,
        "volumeGroup": {"dataMount": {"display": "/System/Volumes/Data"}}
    });
    let kept = CliOutbound
        .filter("scope.review", &caller(), review)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        kept,
        json!({
            "reviewFingerprint": "abc",
            "targets": [{"access": "accessible", "reasonKey": null}, null]
        })
    );
    Ok(())
}

#[test]
fn a_call_the_scan_command_does_not_make_is_refused() {
    for name in ["browse.open", "history.list", "space.total"] {
        assert!(
            CliOutbound.filter(name, &caller(), json!({})).is_err(),
            "{name}"
        );
    }
}

#[test]
fn a_status_keeps_only_the_counts_of_its_terminal() -> Result<(), String> {
    let status = json!({
        "jobRef": "j", "kind": "scan.metadata", "state": "done", "sequence": 3,
        "resultVersion": "v", "reasonKey": null,
        "scanResult": {"lineageId": "l", "sequence": 1},
        "terminal": {
            "state": "done", "sequence": 3, "completed": 2, "overwritten": 0,
            "failed": 1, "skipped": 4, "unprocessed": 0, "historyRecorded": false,
            "unprocessedSummary": "C:\\Secret", "resultVersion": "v"
        }
    });
    let kept = CliOutbound
        .filter("job.status", &caller(), status)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        kept,
        json!({
            "jobRef": "j", "state": "done", "resultVersion": "v",
            "terminal": {"completed": 2, "failed": 1, "skipped": 4}
        })
    );
    Ok(())
}

#[test]
fn a_start_and_a_cancel_keep_only_what_the_command_reads() -> Result<(), String> {
    let start = json!({
        "jobRef": "j", "existing": false, "scopeVersion": "s",
        "scopeFingerprint": "f", "lineageId": "l"
    });
    let cancel = json!({"outcome": "requested", "state": "running", "sequence": 2});
    let filter = |name, value| {
        CliOutbound
            .filter(name, &caller(), value)
            .map_err(|error| format!("{error:?}"))
    };
    assert_eq!(
        filter("scan.start", start)?,
        json!({"jobRef": "j", "existing": false})
    );
    assert_eq!(
        filter("job.cancel", cancel)?,
        json!({"outcome": "requested"})
    );
    Ok(())
}
