//! Digest, channel and routing behavior of the product commands.
use super::*;

#[test]
fn production_ports_hash_the_running_executable() -> Result<(), Box<dyn std::error::Error>> {
    let harness = Harness::from_services(
        ProductServices::system(official()),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(Mutex::new(Vec::new())),
    )?;
    let expected: String = Sha256::digest(std::fs::read(std::env::current_exe()?)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let body = harness.call("app.executable_digest", json!({"correlationId":ID}))?["body"].clone();
    assert_eq!(body["sha256"], expected);
    let info = harness.call("app.product_info", json!({}))?["body"].clone();
    assert!(info["installForm"].is_string());
    let support = harness.call("app.support_info", json!({}))?["body"].clone();
    assert_eq!(support["environment"]["osFamily"], std::env::consts::OS);
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn digest_larger_than_one_read_chunk_covers_every_byte() -> Result<(), Box<dyn std::error::Error>> {
    let length = 64 * 1024 * 3 + 12_345;
    let harness = Harness::new(
        official(),
        true,
        InstallForm::Deb,
        Executable::Generated(length),
    )?;
    let body = harness.call("app.executable_digest", json!({"correlationId":ID}))?["body"].clone();
    assert_eq!(body["sha256"], hex(&generated(length)));
    assert_eq!(body["reasonKey"], Value::Null);
    Ok(())
}

#[test]
fn failed_digest_is_not_cached_and_the_next_request_retries()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = Harness::new(official(), true, InstallForm::Deb, Executable::FailsOnce)?;
    let first = harness.call("app.executable_digest", json!({"correlationId":ID}))?["body"].clone();
    assert_eq!(first["sha256"], Value::Null);
    assert_eq!(first["reasonKey"], "product.digest_unavailable");
    for _ in 0..2 {
        let again =
            harness.call("app.executable_digest", json!({"correlationId":ID}))?["body"].clone();
        assert_eq!(again["sha256"], hex(b"abc"));
        assert_eq!(again["reasonKey"], Value::Null);
    }
    assert_eq!(
        harness.opens.load(Ordering::SeqCst),
        2,
        "one failure, one retry, then the success is cached"
    );
    Ok(())
}

#[test]
fn digest_is_hashed_on_a_worker_thread_and_never_blocks_other_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let gate: Gate = Arc::new((Mutex::new(false), Condvar::new()));
    let harness = Harness::with_wait(
        official(),
        true,
        InstallForm::Deb,
        Executable::Blocked(gate.clone()),
        Some(Duration::from_millis(50)),
    )?;
    let pending =
        harness.call("app.executable_digest", json!({"correlationId":ID}))?["body"].clone();
    assert_eq!(pending["sha256"], Value::Null);
    assert_eq!(pending["reasonKey"], "product.digest_unavailable");
    let info = harness.call("app.product_info", json!({}))?;
    assert!(info.get("error").is_none(), "{info}");
    release(&gate);
    let done = harness.call("app.executable_digest", json!({"correlationId":ID}))?["body"].clone();
    assert_eq!(done["sha256"], hex(b"abc"));
    assert_eq!(harness.opens.load(Ordering::SeqCst), 1, "same attempt");
    let threads = harness.threads.lock().map_err(|_| "poisoned")?;
    assert_eq!(threads.len(), 1);
    assert_ne!(threads[0], std::thread::current().id());
    Ok(())
}

#[test]
fn product_commands_are_available_to_the_main_window_only() -> Result<(), Box<dyn std::error::Error>>
{
    let harness = Harness::new(
        official(),
        true,
        InstallForm::Deb,
        Executable::Bytes(b"abc"),
    )?;
    let correlation: Id = serde_json::from_value(json!(ID))?;
    for name in COMMANDS {
        let body = if name == "app.notices" {
            json!({"kind":"LICENSE"})
        } else if name == "app.executable_digest" {
            json!({"correlationId":ID})
        } else {
            json!({})
        };
        let gui = harness.call(name, body.clone())?;
        assert!(gui.get("error").is_none(), "{name}: {gui}");
        for channel in [Channel::Cli, Channel::Mcp] {
            let envelope: CallEnvelope = serde_json::from_value(json!({
                "name":name, "contractVersion":1, "correlationId":ID, "body":body,
            }))?;
            let caller = CallerCtx::new(channel, correlation.clone(), None);
            let reply = serde_json::to_value(harness.engine.call(envelope, &caller))?;
            assert_eq!(
                reply["error"]["reasonKey"], "command.not_provided",
                "{name} {channel:?}: {reply}"
            );
            assert!(reply.get("body").is_none_or(Value::is_null), "{reply}");
        }
    }
    assert_eq!(
        harness.opens.load(Ordering::SeqCst),
        1,
        "denied callers must not start or reuse another hash"
    );
    Ok(())
}

#[test]
fn malformed_product_info_json_stops_startup_with_the_metadata_key() {
    for json in ["", "{", "[]", r#"{"build":null}"#, "null"] {
        let error = parse_embedded(json).err().map(|error| error.to_string());
        assert_eq!(
            error.as_deref(),
            Some("product.metadata_invalid"),
            "{json:?}"
        );
    }
    // The public entry parses before it creates any window, so this is safe to call.
    let error = crate::run_with_product_info_json("{")
        .err()
        .map(|e| e.to_string());
    assert_eq!(error.as_deref(), Some("product.metadata_invalid"));
}

#[test]
fn build_matrix_revision_that_differs_from_the_matrix_is_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    let mut info = official();
    info.build.support_matrix_revision = Some("sm-8".into());
    let harness = Harness::new(info, true, InstallForm::Deb, Executable::OpenFails)?;
    let product = harness.call("app.product_info", json!({}))?["body"].clone();
    assert_eq!(product["supportMatrixRevision"], Value::Null);
    assert_eq!(product["supportMatrixAsOf"], Value::Null);
    let support = harness.call("app.support_info", json!({}))?["body"].clone();
    assert_eq!(support["combinationStatus"], "unknown");
    assert_eq!(support["reasonKey"], "support.matrix_unavailable");
    Ok(())
}

#[cfg(windows)]
#[test]
fn only_the_digest_command_leaves_the_ordered_lane() {
    use du_platform_os::webview::DispatchLane::{Ordered, Volume};
    for name in COMMANDS {
        let message = json!({"command":"du_call", "args":{"envelope":{"name":name}}}).to_string();
        let expected = if name == "app.executable_digest" {
            Volume
        } else {
            Ordered
        };
        assert_eq!(dispatch_lane(&message), expected, "{name}");
    }
}
