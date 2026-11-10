use super::*;
use crate::desktop::SharedTableLimit;

#[test]
fn scan_admission_reads_current_limit_once_and_running_job_does_not_reread()
-> Result<(), Box<dyn std::error::Error>> {
    let current = Arc::new(AtomicUsize::new(100_000));
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let source = {
        let current = current.clone();
        let reads = reads.clone();
        let observed = observed.clone();
        Arc::new(move || {
            reads.fetch_add(1, Ordering::SeqCst);
            let value = current.load(Ordering::SeqCst);
            if let Ok(mut observed) = observed.lock() {
                observed.push(value);
            }
            SharedTableLimit::new(value).unwrap_or_default()
        })
    };
    let ScanHarness {
        engine,
        release,
        terminal,
        versions,
        ..
    } = fixture::scan_harness_with_options(None, None, Some(source))?;
    let path: Vec<u8> = "C:\\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({"includes":[{"encoding":"windows_utf16le","raw":PathBytes::from_bytes(&path),"display":"fixture","lossy":false}],"exclusions":[],"boundary":"current_volume","includedMounts":[],"source":"gui"});
    let gui = |name, body| call(&engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let review: ScopeReview =
        serde_json::from_value(gui("scope.review", json!({"draft":draft,"requestSeq":1}))?.body)?;
    let first: ScanStart = serde_json::from_value(
        gui(
            "scan.start",
            json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
        )?
        .body,
    )?;
    assert!(!first.existing);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    current.store(10_000_000, Ordering::SeqCst);
    release.send(())?;
    let (summary, _) = scan_finished(&terminal)?;
    let old = versions
        .pin_current(&summary.result_version_id.ok_or("missing version")?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    let second: ScanStart = serde_json::from_value(
        gui(
            "scan.start",
            json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
        )?
        .body,
    )?;
    assert!(!second.existing);
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(
        *observed.lock().map_err(|_| "poisoned")?,
        vec![100_000, 10_000_000]
    );
    release.send(())?;
    let (summary, _) = scan_finished(&terminal)?;
    assert_ne!(
        old.version_id(),
        &summary.result_version_id.ok_or("missing second version")?
    );
    assert_eq!(old.items().len(), 2);
    Ok(())
}

#[test]
fn limit_accepts_only_published_bounds() {
    assert_eq!(SharedTableLimit::default().entries(), 1_000_000);
    for value in [100_000, 10_000_000] {
        assert_eq!(
            SharedTableLimit::new(value).map(SharedTableLimit::entries),
            Some(value)
        );
    }
    for value in [0, 99_999, 10_000_001, usize::MAX] {
        assert_eq!(SharedTableLimit::new(value), None);
    }
}
