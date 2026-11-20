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
        "notices": [],
        "targets": [{"path": {"display": "C:\\Secret"}, "access": "accessible", "reasonKey": null}],
        "reviewFingerprint": "abc",
        "observedAt": "2026-11-20T00:00:00Z",
        "requestSeq": 1
    });
    let kept = CliOutbound
        .filter("scope.review", &caller(), review)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        kept,
        json!({
            "reviewFingerprint": "abc",
            "requestSeq": 1,
            "notices": [],
            "targets": [{"access": "accessible", "reasonKey": null}]
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
fn a_status_keeps_its_terminal_and_drops_the_lineage() -> Result<(), String> {
    let status = json!({
        "jobRef": "j", "kind": "scan.metadata", "state": "done", "sequence": 3,
        "resultVersion": "v", "reasonKey": null,
        "scanResult": {"lineageId": "l", "sequence": 1},
        "terminal": {"state": "done", "completed": 2}
    });
    let kept = CliOutbound
        .filter("job.status", &caller(), status)
        .map_err(|error| format!("{error:?}"))?;
    assert!(kept.get("scanResult").is_none());
    assert_eq!(kept["terminal"]["completed"], 2);
    Ok(())
}
