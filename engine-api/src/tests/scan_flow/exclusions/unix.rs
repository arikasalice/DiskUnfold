//! Unix scopes review and start with byte-exact exclusions and no comparison provider.
use super::*;
use du_core_contracts::{
    analysis_ipc::{ScopeDraft, ScopeExclusion},
    scan_observation::FilterDecision::{Admit, Exclude},
};

struct UnixRoots {
    opens: AtomicUsize,
}

impl ScopeRootPort for UnixRoots {
    fn open_root(&self, path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        self.opens.fetch_add(1, Ordering::Relaxed);
        Ok(OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(path),
                display: String::from_utf8_lossy(path).into_owned(),
                lossy: false,
            },
            volume_id: b"linux:8:1:0".to_vec(),
            file_id: path.to_vec(),
            directory: Box::new(Empty),
        })
    }
}

struct Empty;

impl DirectoryObservations for Empty {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}

impl AnchoredDirectory for Empty {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "os.query_failed".into(),
            os_code: None,
        })
    }
}

fn unix(raw: &[u8]) -> serde_json::Value {
    json!({"encoding": "unix", "raw": PathBytes::from_bytes(raw), "display": "untrusted", "lossy": false})
}

fn draft(includes: &[&[u8]], exclusions: serde_json::Value) -> serde_json::Value {
    let includes: Vec<_> = includes.iter().map(|raw| unix(raw)).collect();
    json!({"includes": includes, "exclusions": exclusions, "boundary": "current_volume", "includedMounts": [], "source": "gui"})
}

fn review_engine(roots: Arc<UnixRoots>) -> Result<Engine, Box<dyn std::error::Error>> {
    let mut builder = EngineBuilder::default();
    crate::scope::register_review(&mut builder, roots, Arc::new(Clock));
    Ok(builder.build()?)
}

#[test]
fn unix_review_binds_byte_exact_rules_to_start_without_a_provider()
-> Result<(), Box<dyn std::error::Error>> {
    let roots = Arc::new(UnixRoots {
        opens: AtomicUsize::new(0),
    });
    let engine = review_engine(roots.clone())?;
    let review = |draft: &serde_json::Value| {
        call(
            &engine,
            "scope.review",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({"draft": draft, "requestSeq": 1}),
        )
    };
    let requested = draft(
        &[b"/data"],
        json!([{"kind": "path", "path": unix(b"/data/Private")}, {"kind": "name_pattern", "pattern": "*.tmp"}]),
    );
    let reviewed: ScopeReview = serde_json::from_value(review(&requested)?.body)?;
    assert!(
        !reviewed
            .notices
            .iter()
            .any(|notice| notice == "scope.exclusion_ineffective")
    );
    assert_eq!(
        reviewed.scope.exclusions[1],
        ScopeExclusion::NamePattern {
            pattern: "*.tmp".into()
        }
    );
    let mut spelled = requested.clone();
    spelled["exclusions"][0]["path"] = unix(b"/data//Private/");
    let same: ScopeReview = serde_json::from_value(review(&spelled)?.body)?;
    assert_eq!(same.review_fingerprint, reviewed.review_fingerprint);
    let parsed: ScopeDraft = serde_json::from_value(requested)?;
    let verified =
        crate::scope::verify(&parsed, &reviewed.review_fingerprint, &*roots, None, false)
            .map_err(|error| format!("{error:?}"))?;
    let hashed = verified
        .scope_fingerprint
        .retained_comparison_input()
        .ok_or("missing comparison inputs")?;
    assert_eq!(hashed.exclusions.len(), 2);
    let filter = verified.filter.ok_or("missing filter")?;
    assert_eq!(filter(0, &[], b"Private"), Exclude);
    assert_eq!(filter(0, &[], b"private"), Admit);
    assert_eq!(filter(0, &[b"nested".to_vec()], b"a.tmp"), Exclude);
    assert_eq!(filter(0, &[b"nested".to_vec()], b"a.TMP"), Admit);
    let mut forged = parsed.clone();
    forged.exclusions[0] = ScopeExclusion::Path {
        path: serde_json::from_value(unix(b"/data/private"))?,
    };
    assert_eq!(
        crate::scope::verify(&forged, &reviewed.review_fingerprint, &*roots, None, false).err(),
        Some(EngineError::reason("scope.changed"))
    );
    Ok(())
}

#[test]
fn unix_rules_remove_roots_and_rejected_rules_open_nothing()
-> Result<(), Box<dyn std::error::Error>> {
    let roots = Arc::new(UnixRoots {
        opens: AtomicUsize::new(0),
    });
    let engine = review_engine(roots.clone())?;
    let review = |draft: serde_json::Value| {
        call(
            &engine,
            "scope.review",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({"draft": draft, "requestSeq": 1}),
        )
    };
    let reviewed: ScopeReview = serde_json::from_value(
        review(draft(
            &[b"/data/a", b"/data/b"],
            json!([{"kind": "path", "path": unix(b"/data/a")}]),
        ))?
        .body,
    )?;
    assert!(
        reviewed
            .notices
            .iter()
            .any(|notice| notice == "scope.excluded")
    );
    let kept: Vec<_> = reviewed
        .scope
        .includes
        .iter()
        .map(|path| path.raw.bytes())
        .collect::<Result<_, _>>()?;
    assert_eq!(kept, [b"/data/b".to_vec()]);
    let reason = |draft| -> Result<String, Box<dyn std::error::Error>> {
        Ok(review(draft)?.error.ok_or("rule accepted")?.reason_key)
    };
    for rules in [
        json!([{"kind": "path", "path": unix(b"/")}]),
        json!([{"kind": "name_pattern", "pattern": "cache"}]),
    ] {
        assert_eq!(reason(draft(&[b"/srv/cache"], rules))?, "scope.empty");
    }
    let opened = roots.opens.load(Ordering::Relaxed);
    let wide: Vec<u8> = "C:\\data\\x"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let windows = json!({"encoding": "windows_utf16le", "raw": PathBytes::from_bytes(&wide), "display": "C:\\data\\x", "lossy": false});
    for (rule, expected) in [
        (
            json!({"kind": "path", "path": unix(b"data/x")}),
            "path.unresolvable",
        ),
        (
            json!({"kind": "path", "path": unix(b"/data/../x")}),
            "scope.exclude_invalid",
        ),
        (
            json!({"kind": "name_pattern", "pattern": "a/b"}),
            "scope.exclude_invalid",
        ),
        (
            json!({"kind": "path", "path": windows}),
            "scope.exclude_invalid",
        ),
    ] {
        assert_eq!(
            reason(draft(&[b"/data"], json!([rule.clone()])))?,
            expected,
            "{rule}"
        );
    }
    assert_eq!(roots.opens.load(Ordering::Relaxed), opened);
    Ok(())
}
