//! #222: the command line reviews, starts, watches and cancels only scopes it
//! names as its own; the screen keeps its own source, and MCP stays closed.
use super::*;

fn draft(source: &str) -> serde_json::Value {
    let path: Vec<u8> = "C:\\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    json!({
        "includes": [{
            "encoding": "windows_utf16le",
            "raw": PathBytes::from_bytes(&path),
            "display": "C:\\Fixture",
            "lossy": false
        }],
        "exclusions": [],
        "boundary": "current_volume",
        "includedMounts": [],
        "source": source
    })
}

fn refusal(response: &ResultEnvelope) -> Result<&str, Box<dyn std::error::Error>> {
    let error = response.error.as_ref().ok_or("expected a refusal")?;
    // An error envelope carries keys and opaque references only: no path.
    assert!(!serde_json::to_string(response)?.contains("Fixture"));
    Ok(&error.reason_key)
}

#[test]
fn each_channel_reviews_and_starts_only_the_scope_source_it_names()
-> Result<(), Box<dyn std::error::Error>> {
    let ScanHarness {
        engine,
        release,
        terminal,
        ..
    } = fixture::scan_harness_with_cli()?;
    let send = |name, channel, body| call(&engine, name, channel, ENVELOPE_VERSION, body);
    let review = |channel, source| {
        send(
            "scope.review",
            channel,
            json!({"draft": draft(source), "requestSeq": 1}),
        )
    };
    for (channel, source) in [
        (Channel::Gui, "cli"),
        (Channel::Gui, "recent"),
        (Channel::Cli, "gui"),
        (Channel::Cli, "recent"),
    ] {
        let refused = review(channel, source)?;
        assert_eq!(
            refusal(&refused)?,
            "scope.target_unavailable",
            "{channel:?} {source}"
        );
    }
    let gui: ScopeReview = serde_json::from_value(review(Channel::Gui, "gui")?.body)?;
    let cli: ScopeReview = serde_json::from_value(review(Channel::Cli, "cli")?.body)?;
    let start = |channel, source, fingerprint: &str| {
        send(
            "scan.start",
            channel,
            json!({"draft": draft(source), "reviewFingerprint": fingerprint}),
        )
    };
    // A start repeats the check, whatever fingerprint it carries.
    let crossed = start(Channel::Gui, "cli", &cli.review_fingerprint)?;
    assert_eq!(refusal(&crossed)?, "scope.target_unavailable");
    let crossed = start(Channel::Cli, "gui", &gui.review_fingerprint)?;
    assert_eq!(refusal(&crossed)?, "scope.target_unavailable");

    let receipt: ScanStart =
        serde_json::from_value(start(Channel::Cli, "cli", &cli.review_fingerprint)?.body)?;
    let job = json!({"jobRef": receipt.job_ref});
    let status = send("job.status", Channel::Cli, job.clone())?;
    assert!(status.error.is_none());
    assert_eq!(status.body["jobRef"], json!(receipt.job_ref));
    let cancel = send("job.cancel", Channel::Cli, job.clone())?;
    assert!(cancel.error.is_none());
    release.send(())?;
    let (summary, _) = scan_finished(&terminal)?;
    assert_eq!(summary.job_id, receipt.job_ref);

    for (name, body) in [
        (
            "scope.review",
            json!({"draft": draft("gui"), "requestSeq": 2}),
        ),
        (
            "scan.start",
            json!({"draft": draft("gui"), "reviewFingerprint": gui.review_fingerprint}),
        ),
        ("job.status", job.clone()),
        ("job.cancel", job.clone()),
    ] {
        let closed = send(name, Channel::Mcp, body)?;
        assert_eq!(refusal(&closed)?, "command.not_provided", "{name}");
    }
    Ok(())
}
