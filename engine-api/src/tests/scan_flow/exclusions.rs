use super::*;
mod invalid;
use crate::scope::exclusion::{ComparisonPath, PathComparison};
use du_core_contracts::analysis_ipc::ScopeReview;

struct ProvenComparison;
impl PathComparison for ProvenComparison {
    fn path(&self, path: &OsPath) -> Option<ComparisonPath> {
        let raw = path.raw.bytes().ok()?;
        let (pairs, remainder) = raw.as_chunks::<2>();
        if !remainder.is_empty() {
            return None;
        }
        let units = pairs
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect::<Vec<_>>();
        let text = String::from_utf16(&units).ok()?.to_ascii_lowercase();
        let volume = if text.starts_with("c:") {
            vec![1]
        } else {
            vec![2]
        };
        let components = text
            .split('\\')
            .map(|part| part.as_bytes().to_vec())
            .collect();
        let raw: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        Some(ComparisonPath {
            volume,
            components,
            serialized_key: OsPath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(&raw),
                display: text,
                lossy: false,
            },
        })
    }
    fn component(&self, _: &ComparisonPath, _: &[Vec<u8>], raw: &[u8]) -> Option<Vec<u8>> {
        let (pairs, rest) = raw.as_chunks::<2>();
        if !rest.is_empty() {
            return None;
        }
        String::from_utf16(
            &pairs
                .iter()
                .map(|p| u16::from_le_bytes(*p))
                .collect::<Vec<_>>(),
        )
        .ok()
        .map(|s| s.to_ascii_lowercase().into_bytes())
    }
}

fn native(path: &str) -> serde_json::Value {
    let bytes: Vec<_> = path.encode_utf16().flat_map(u16::to_le_bytes).collect();
    json!({"encoding": "windows_utf16le", "raw": PathBytes::from_bytes(&bytes), "display": "untrusted", "lossy": false})
}

#[test]
fn original_rules_stay_paired_with_single_observed_comparison_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::scope::exclusion::prepare;
    use du_core_contracts::scan_observation::FilterDecision;
    let path = |s: &str| -> Result<OsPath, serde_json::Error> { serde_json::from_value(native(s)) };
    let roots = vec![(path("C:\\Fixture")?, vec![1])];
    let rules = vec![
        path("C:\\FIXTURE\\F")?,
        path("D:\\Other\\X")?,
        path("C:\\FIXTURE\\F")?,
    ];
    let frozen = prepare(Arc::new(ProvenComparison), &roots, &rules).ok_or("missing evidence")?;
    assert!(frozen.ineffective_rule);
    assert_eq!(frozen.rules.len(), 3);
    for (bound, original) in frozen.rules.iter().zip(&rules) {
        assert_eq!(bound.original(), original);
        let expected = ProvenComparison
            .path(original)
            .ok_or("missing expected key")?;
        assert_eq!(bound.comparison().serialized_key, expected.serialized_key);
        assert_eq!(bound.comparison().volume, expected.volume);
        assert_eq!(bound.comparison().components, expected.components);
    }
    assert_eq!(
        (frozen.filter)(0, &[], &native_component("F")),
        FilterDecision::Exclude
    );
    assert_eq!(
        (frozen.filter)(0, &[], &native_component("else")),
        FilterDecision::Admit
    );
    let reversed = rules.iter().rev().cloned().collect::<Vec<_>>();
    let again = prepare(Arc::new(ProvenComparison), &roots, &reversed).ok_or("missing evidence")?;
    let keys = |values: &[crate::scope::exclusion::VerifiedExclusion]| {
        let mut keys = values
            .iter()
            .map(|value| value.comparison().serialized_key.raw.bytes())
            .collect::<Result<Vec<_>, _>>()?;
        keys.sort();
        Ok::<_, Box<dyn std::error::Error>>(keys)
    };
    assert_eq!(keys(&frozen.rules)?, keys(&again.rules)?);
    Ok(())
}

#[test]
fn repeated_and_ineffective_rules_keep_review_binding_when_reordered()
-> Result<(), Box<dyn std::error::Error>> {
    let ScanHarness { engine, .. } =
        fixture::scan_harness_with_comparison(None, Some(Arc::new(ProvenComparison)))?;
    let gui = |name, body| call(&engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let mut draft = json!({"includes":[native("C:\\Fixture")],"exclusions":[{"kind":"path","path":native("C:\\Fixture\\F")},{"kind":"path","path":native("D:\\Other\\X")},{"kind":"path","path":native("C:\\Fixture\\F")}],"boundary":"current_volume","includedMounts":[],"source":"gui"});
    let review: ScopeReview =
        serde_json::from_value(gui("scope.review", json!({"draft":draft,"requestSeq":1}))?.body)?;
    assert!(
        review
            .notices
            .iter()
            .any(|notice| notice == "scope.exclusion_ineffective")
    );
    draft["exclusions"]
        .as_array_mut()
        .ok_or("invalid rules")?
        .rotate_left(1);
    let result = gui(
        "scan.start",
        json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
    )?;
    assert!(result.error.is_none());
    let _: ScanStart = serde_json::from_value(result.body)?;
    Ok(())
}

struct ChangingRootKey {
    enabled: std::sync::atomic::AtomicBool,
    root_reads: std::sync::atomic::AtomicUsize,
    change_after: usize,
}

impl PathComparison for ChangingRootKey {
    fn path(&self, path: &OsPath) -> Option<ComparisonPath> {
        let mut result = ProvenComparison.path(path)?;
        if self.enabled.load(Ordering::Acquire)
            && result.components.len() == 2
            && self.root_reads.fetch_add(1, Ordering::AcqRel) >= self.change_after
        {
            result.serialized_key = serde_json::from_value(native("C:\\Different")).ok()?;
        }
        Some(result)
    }

    fn component(&self, root: &ComparisonPath, parent: &[Vec<u8>], raw: &[u8]) -> Option<Vec<u8>> {
        ProvenComparison.component(root, parent, raw)
    }
}

#[test]
fn start_rejects_root_key_that_changes_after_filter_preparation()
-> Result<(), Box<dyn std::error::Error>> {
    let comparison = Arc::new(ChangingRootKey {
        enabled: std::sync::atomic::AtomicBool::new(false),
        root_reads: std::sync::atomic::AtomicUsize::new(0),
        change_after: 1,
    });
    let ScanHarness { engine, roots, .. } =
        fixture::scan_harness_with_comparison(None, Some(comparison.clone()))?;
    let gui = |name, body| call(&engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let draft = json!({"includes":[native("C:\\Fixture")],"exclusions":[{"kind":"path","path":native("C:\\Fixture\\F")}],"boundary":"current_volume","includedMounts":[],"source":"gui"});
    let review: ScopeReview =
        serde_json::from_value(gui("scope.review", json!({"draft":draft,"requestSeq":1}))?.body)?;
    comparison.enabled.store(true, Ordering::Release);
    let response = gui(
        "scan.start",
        json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
    )?;
    assert_eq!(
        response
            .error
            .ok_or("inconsistent evidence accepted")?
            .reason_key,
        "scope.changed"
    );
    assert_eq!(roots.reads.load(Ordering::Acquire), 0);
    Ok(())
}

#[test]
fn start_rejects_conflicting_duplicate_canonical_witness() -> Result<(), Box<dyn std::error::Error>>
{
    let comparison = Arc::new(ChangingRootKey {
        enabled: std::sync::atomic::AtomicBool::new(false),
        root_reads: std::sync::atomic::AtomicUsize::new(0),
        change_after: 2,
    });
    let ScanHarness { engine, roots, .. } =
        fixture::scan_harness_with_comparison(None, Some(comparison.clone()))?;
    let gui = |name, body| call(&engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let draft = json!({"includes":[native("C:\\Fixture"),native("C:\\Fixture")],"exclusions":[{"kind":"path","path":native("C:\\Fixture\\F")}],"boundary":"current_volume","includedMounts":[],"source":"gui"});
    let review: ScopeReview =
        serde_json::from_value(gui("scope.review", json!({"draft":draft,"requestSeq":1}))?.body)?;
    comparison.enabled.store(true, Ordering::Release);
    let result = gui(
        "scan.start",
        json!({"draft":draft,"reviewFingerprint":review.review_fingerprint}),
    )?;
    assert_eq!(
        result
            .error
            .ok_or("duplicate conflict accepted")?
            .reason_key,
        "scope.changed"
    );
    assert_eq!(comparison.root_reads.load(Ordering::Acquire), 3);
    assert_eq!(roots.reads.load(Ordering::Acquire), 0);
    Ok(())
}

#[test]
fn reviewed_exclusions_are_bound_to_start_and_filter_the_job()
-> Result<(), Box<dyn std::error::Error>> {
    let ScanHarness {
        engine,
        roots,
        terminal,
        versions,
        ..
    } = fixture::scan_harness_with_comparison(None, Some(Arc::new(ProvenComparison)))?;
    let gui = |name, body| call(&engine, name, Channel::Gui, ENVELOPE_VERSION, body);
    let draft = json!({"includes": [native("C:\\Fixture")], "exclusions": [{"kind":"path", "path":native("C:\\FIXTURE\\F")}], "boundary":"current_volume", "includedMounts":[], "source":"gui"});
    let review: ScopeReview =
        serde_json::from_value(gui("scope.review", json!({"draft": draft, "requestSeq": 1}))?.body)
            .map_err(|error| format!("{error:?}"))?;
    let requested: du_core_contracts::analysis_ipc::ScopeDraft =
        serde_json::from_value(draft.clone())?;
    assert_eq!(review.scope.exclusions, requested.exclusions);
    let mut equivalent = draft.clone();
    equivalent["includes"][0] = native("c:\\fixture");
    equivalent["exclusions"][0]["path"] = native("c:\\fixture\\f");
    let same: ScopeReview = serde_json::from_value(
        gui(
            "scope.review",
            json!({"draft": equivalent, "requestSeq": 3}),
        )?
        .body,
    )?;
    assert_eq!(review.review_fingerprint, same.review_fingerprint);
    let mut ordered = draft.clone();
    ordered["exclusions"]
        .as_array_mut()
        .ok_or("invalid exclusions")?
        .push(json!({"kind":"path", "path":native("D:\\Other\\x")}));
    let first: ScopeReview = serde_json::from_value(
        gui("scope.review", json!({"draft": ordered, "requestSeq": 4}))?.body,
    )?;
    ordered["exclusions"]
        .as_array_mut()
        .ok_or("invalid exclusions")?
        .reverse();
    let second: ScopeReview = serde_json::from_value(
        gui("scope.review", json!({"draft": ordered, "requestSeq": 5}))?.body,
    )?;
    assert_eq!(first.review_fingerprint, second.review_fingerprint);
    assert!(
        first
            .notices
            .iter()
            .any(|notice| notice == "scope.exclusion_ineffective")
    );
    assert!(
        !review
            .notices
            .iter()
            .any(|notice| notice == "scope.exclusion_ineffective")
    );
    let mut forged = draft.clone();
    forged["exclusions"][0]["path"] = native("C:\\Fixture\\other");
    assert_eq!(
        gui(
            "scan.start",
            json!({"draft": forged, "reviewFingerprint": review.review_fingerprint})
        )?
        .error
        .ok_or("forged accepted")?
        .reason_key,
        "scope.changed"
    );
    roots.file_id.store(9, Ordering::Release);
    assert_eq!(
        gui(
            "scan.start",
            json!({"draft": draft, "reviewFingerprint": review.review_fingerprint})
        )?
        .error
        .ok_or("stale accepted")?
        .reason_key,
        "scope.changed"
    );
    roots.file_id.store(1, Ordering::Release);
    let receipt: ScanStart = serde_json::from_value(
        gui(
            "scan.start",
            json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
        )?
        .body,
    )?;
    let (summary, _) = scan_finished(&terminal)?;
    assert_eq!(summary.job_id, receipt.job_ref);
    let result = versions
        .pin()
        .map_err(|e| format!("{e:?}"))?
        .ok_or("missing result")?;
    assert_eq!(result.items().len(), 1);
    assert_eq!(roots.reads.load(Ordering::Acquire), 0);
    let mut all = draft.clone();
    all["exclusions"][0]["path"] = native("C:\\fixture");
    assert_eq!(
        gui("scope.review", json!({"draft": all, "requestSeq": 2}))?
            .error
            .ok_or("empty accepted")?
            .reason_key,
        "scope.empty"
    );
    Ok(())
}

#[test]
fn root_order_cross_volume_and_rule_order_use_proven_keys() -> Result<(), Box<dyn std::error::Error>>
{
    use crate::scope::exclusion::prepare;
    use du_core_contracts::scan_observation::FilterDecision;
    let path = |s: &str| -> Result<OsPath, serde_json::Error> { serde_json::from_value(native(s)) };
    let roots = vec![
        (path("D:\\Other")?, vec![2]),
        (path("C:\\Fixture")?, vec![1]),
    ];
    let rules = vec![
        path("C:\\FIXTURE\\child\\private")?,
        path("D:\\Other\\skip")?,
    ];
    let policy = prepare(Arc::new(ProvenComparison), &roots, &rules).ok_or("no policy")?;
    assert_eq!(policy.roots, [1, 0]);
    assert!(!policy.ineffective_rule);
    let child = native_component("CHILD");
    let private = native_component("PRIVATE");
    let neighbor = native_component("public");
    assert_eq!(
        (policy.filter)(0, std::slice::from_ref(&child), &private),
        FilterDecision::Exclude
    );
    assert_eq!(
        (policy.filter)(0, &[child], &neighbor),
        FilterDecision::Admit
    );
    assert_eq!(
        (policy.filter)(1, &[], &native_component("skip")),
        FilterDecision::Exclude
    );
    assert_eq!(
        (policy.filter)(1, &[], &native_component("private")),
        FilterDecision::Admit
    );
    assert_eq!((policy.filter)(0, &[], &[0]), FilterDecision::Undecidable);
    assert_eq!(
        (policy.filter)(0, &[native_component("public")], &[0]),
        FilterDecision::Admit
    );
    assert_eq!(
        (policy.filter)(0, &[native_component("child")], &[0]),
        FilterDecision::Undecidable
    );
    assert_eq!(
        (policy.filter)(
            0,
            &[native_component("child"), native_component("private")],
            &[0]
        ),
        FilterDecision::Exclude
    );
    let policy = prepare(
        Arc::new(ProvenComparison),
        &roots,
        &[path("c:\\FIXTURE")?, path("D:\\OtherSibling")?],
    )
    .ok_or("no policy")?;
    assert_eq!(policy.roots, [0]);
    assert!(policy.ineffective_rule);
    Ok(())
}

fn native_component(value: &str) -> Vec<u8> {
    value.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

struct AncestorRoots(Arc<Roots>);
struct AncestorDirectory {
    inner: Box<dyn AnchoredDirectory>,
    ancestors: Vec<Vec<u8>>,
}
impl DirectoryObservations for AncestorDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.inner.next_batch()
    }
}
impl AnchoredDirectory for AncestorDirectory {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        self.inner.open_child(entry)
    }
    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        Some(&self.ancestors)
    }
}
impl ScopeRootPort for AncestorRoots {
    fn open_root(&self, path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let mut root = self.0.open_root(path)?;
        let nested = path == native_component("C:\\Fixture\\Nested");
        let ancestors = if nested {
            vec![root.file_id.clone()]
        } else {
            Vec::new()
        };
        if nested {
            root.file_id = 99usize.to_le_bytes().to_vec();
        }
        root.directory = Box::new(AncestorDirectory {
            inner: root.directory,
            ancestors,
        });
        Ok(root)
    }
}

#[test]
fn exclusion_inside_absorbed_root_stays_bound_to_retained_root()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::{analysis_ipc::ScopeDraft, scan_observation::FilterDecision};
    let (release, receiver) = mpsc::channel();
    let roots = Arc::new(AncestorRoots(Arc::new(Roots {
        file_id: AtomicUsize::new(1),
        opens: AtomicUsize::new(0),
        reads: Arc::new(AtomicUsize::new(0)),
        read_handles: Arc::new(Mutex::new(Vec::new())),
        release: Arc::new(Mutex::new(receiver)),
        finish_gate: None,
    })));
    let _release = release;
    let comparison: Arc<dyn PathComparison> = Arc::new(ProvenComparison);
    let draft: ScopeDraft = serde_json::from_value(
        json!({"includes":[native("C:\\Fixture\\Nested"), native("C:\\Fixture")], "exclusions":[{"kind":"path","path":native("C:\\Fixture\\Nested\\Private")}], "boundary":"current_volume", "includedMounts":[], "source":"gui"}),
    )?;
    let mut builder = EngineBuilder::default();
    crate::scope::register_review_with_comparison(
        &mut builder,
        roots.clone(),
        Arc::new(Clock),
        comparison.clone(),
    );
    let engine = builder.build()?;
    let review: ScopeReview = serde_json::from_value(
        call(
            &engine,
            "scope.review",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({"draft":draft,"requestSeq":1}),
        )?
        .body,
    )?;
    assert_eq!(review.scope.includes.len(), 1);
    assert!(
        review
            .notices
            .iter()
            .any(|notice| notice == "scope.descendant_absorbed")
    );
    let verified = crate::scope::verify(
        &draft,
        &review.review_fingerprint,
        &*roots,
        Some(comparison),
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(verified.roots.len(), 1);
    let filter = verified.filter.ok_or("missing filter")?;
    assert_eq!(
        filter(
            0,
            &[native_component("Nested")],
            &native_component("Private")
        ),
        FilterDecision::Exclude
    );
    Ok(())
}
