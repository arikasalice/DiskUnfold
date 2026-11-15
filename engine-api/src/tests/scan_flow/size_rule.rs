//! The size aggregation rule is pinned once, when an analysis starts (F03 FR14.HD2.app-host.DD3).
use super::*;
use crate::size_rule::{DEFAULT_SIZE_RULE_ID, SizeRuleSource};
use std::sync::atomic::AtomicUsize;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn draft() -> serde_json::Value {
    let path: Vec<u8> = "C:\\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    json!({"includes":[{"encoding":"windows_utf16le","raw":PathBytes::from_bytes(&path),"display":"fixture","lossy":false}],"exclusions":[],"boundary":"current_volume","includedMounts":[],"source":"gui"})
}

/// A source that names `ids[n]` on its n-th read and the last entry afterwards.
fn scripted(ids: &'static [&'static str]) -> (SizeRuleSource, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
    let source: SizeRuleSource = Arc::new(move || {
        let at = counter.fetch_add(1, Ordering::SeqCst);
        ids.get(at)
            .or(ids.last())
            .copied()
            .unwrap_or_default()
            .to_owned()
    });
    (source, reads)
}

fn review_fingerprint(engine: &Engine, draft: &serde_json::Value) -> Result<String, String> {
    let review: ScopeReview = serde_json::from_value(
        call(
            engine,
            "scope.review",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({"draft":draft,"requestSeq":1}),
        )?
        .body,
    )
    .map_err(|error| error.to_string())?;
    Ok(review.review_fingerprint)
}

fn start(
    engine: &Engine,
    draft: &serde_json::Value,
    fingerprint: &str,
) -> Result<ResultEnvelope, String> {
    call(
        engine,
        "scan.start",
        Channel::Gui,
        ENVELOPE_VERSION,
        json!({"draft":draft,"reviewFingerprint":fingerprint}),
    )
}

#[test]
fn default_source_seals_a_result_that_reports_the_default_rule() -> TestResult {
    let (source, reads) = scripted(&[DEFAULT_SIZE_RULE_ID]);
    let ScanHarness {
        engine,
        release,
        terminal,
        versions,
        ..
    } = fixture::scan_harness_with_size_rule_source(source)?;
    let draft = draft();
    let fingerprint = review_fingerprint(&engine, &draft)?;
    let started = start(&engine, &draft, &fingerprint)?;
    assert!(started.error.is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    release.send(())?;
    let (summary, _) = scan_finished(&terminal)?;
    let version = versions
        .pin_current(&summary.result_version_id.ok_or("missing version")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(version.size_rule().id, "size.agg.subtree-unique");
    assert_eq!(version.size_rule().revision, 1);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn unknown_rule_creates_no_job_and_consumes_no_scope_root() -> TestResult {
    let (source, reads) = scripted(&["size.agg.nope"]);
    let ScanHarness {
        engine,
        roots,
        terminal,
        ..
    } = fixture::scan_harness_with_size_rule_source(source)?;
    let draft = draft();
    let fingerprint = review_fingerprint(&engine, &draft)?;
    let opens = roots.opens.load(Ordering::Acquire);
    let reads_before = roots.reads.load(Ordering::Acquire);
    let rejected = start(&engine, &draft, &fingerprint)?;
    let error = rejected.error.ok_or("an unknown rule was admitted")?;
    assert_eq!(error.code, du_core_contracts::ErrorCode::InvalidArgument);
    assert_eq!(error.reason_key, "space.rule_unknown");
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(roots.opens.load(Ordering::Acquire), opens);
    assert_eq!(roots.reads.load(Ordering::Acquire), reads_before);
    // No job ran, so nothing finished and nothing was published.
    assert!(terminal.recv_timeout(Duration::from_millis(200)).is_err());
    Ok(())
}

#[test]
fn a_changed_source_applies_from_the_next_start() -> TestResult {
    // The first start names the default rule and the later ones name an unknown ID.
    let (source, reads) = scripted(&[DEFAULT_SIZE_RULE_ID, "size.agg.nope"]);
    let ScanHarness {
        engine,
        release,
        terminal,
        versions,
        ..
    } = fixture::scan_harness_with_size_rule_source(source)?;
    let draft = draft();
    let fingerprint = review_fingerprint(&engine, &draft)?;
    let first = start(&engine, &draft, &fingerprint)?;
    assert!(first.error.is_none());
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    // The source changed, but the running scan keeps the rule it pinned.
    release.send(())?;
    let (summary, _) = scan_finished(&terminal)?;
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    let version = versions
        .pin_current(&summary.result_version_id.ok_or("missing version")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(version.size_rule().id, DEFAULT_SIZE_RULE_ID);
    let second = start(&engine, &draft, &fingerprint)?;
    assert_eq!(
        second
            .error
            .ok_or("the changed source was ignored")?
            .reason_key,
        "space.rule_unknown"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    Ok(())
}
