//! Byte-exact Unix rules: admission, root removal and the frozen pre-metadata filter.
use super::{EngineError, Frozen, RuleKeys, check, unix::Pattern, unix::UnixRules};
use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    analysis_ipc::{ScopeBoundary, ScopeDraft, ScopeExclusion, ScopeSource},
    scan_observation::FilterDecision::{Admit, Exclude, Undecidable},
};

fn unix(raw: &[u8]) -> OsPath {
    OsPath {
        encoding: PathEncoding::Unix,
        raw: PathBytes::from_bytes(raw),
        display: "untrusted".into(),
        lossy: false,
    }
}

fn path(raw: &[u8]) -> ScopeExclusion {
    ScopeExclusion::Path { path: unix(raw) }
}

fn name(pattern: &str) -> ScopeExclusion {
    ScopeExclusion::NamePattern {
        pattern: pattern.into(),
    }
}

fn freeze(rules: &[ScopeExclusion], roots: &[&[u8]]) -> Result<Frozen, String> {
    let rules = UnixRules::admit(rules).map_err(|error| format!("{error:?}"))?;
    let roots: Vec<_> = roots
        .iter()
        .map(|raw| (unix(raw), b"linux:8:1:0".to_vec()))
        .collect();
    rules.freeze(&roots).ok_or_else(|| "undecidable".to_owned())
}

fn raw_key(rule: &ScopeExclusion) -> Option<Vec<u8>> {
    match rule {
        ScopeExclusion::Path { path } => path.raw.bytes().ok(),
        ScopeExclusion::NamePattern { .. } => None,
    }
}

#[test]
fn path_rules_compare_raw_names_exactly_below_the_root() -> Result<(), String> {
    let composed = "/data/sub/caf\u{e9}".as_bytes();
    let frozen = freeze(&[path(b"/data/Private"), path(composed)], &[b"/data"])?;
    let filter = &frozen.filter;
    let sub = [b"sub".to_vec()];
    assert_eq!(filter(0, &[], b"Private"), Exclude);
    assert_eq!(filter(0, &[b"Private".to_vec()], b"inner"), Exclude);
    for other in [&b"private"[..], b"PRIVATE", b"Private ", b"\xff"] {
        assert_eq!(filter(0, &[], other), Admit, "{other:?}");
    }
    assert_eq!(filter(0, &sub, "caf\u{e9}".as_bytes()), Exclude);
    // Linux does not normalize names, so a decomposed spelling is another name.
    assert_eq!(filter(0, &sub, "cafe\u{301}".as_bytes()), Admit);
    assert_eq!(filter(0, &sub, b"other"), Admit);
    assert_eq!(filter(1, &[], b"Private"), Undecidable);
    assert_eq!(frozen.roots, [0]);
    assert!(!frozen.ineffective_rule);
    let ScopeExclusion::Path { path: original } = &frozen.rules[0].original else {
        return Err("path rule changed kind".into());
    };
    // The admitted original shows the bytes it holds, never the caller's label.
    assert_eq!(original.display, "/data/Private");
    assert_eq!(
        raw_key(&frozen.rules[0].key),
        Some(b"/data/Private".to_vec())
    );
    Ok(())
}

#[test]
fn spellings_of_one_path_share_a_key_and_roots_follow_key_order() -> Result<(), String> {
    let frozen = freeze(&[path(b"//data///Private/")], &[b"/data/b", b"/data/a"])?;
    assert_eq!(
        raw_key(&frozen.rules[0].key),
        Some(b"/data/Private".to_vec())
    );
    assert_eq!(
        raw_key(&frozen.rules[0].original),
        Some(b"//data///Private/".to_vec())
    );
    assert_eq!(frozen.roots, [1, 0]);
    let keys: Vec<_> = frozen
        .root_keys
        .iter()
        .map(|key| key.raw.bytes().ok())
        .collect();
    assert_eq!(keys, [Some(b"/data/a".to_vec()), Some(b"/data/b".to_vec())]);
    Ok(())
}

#[test]
fn roots_covered_by_a_rule_or_named_by_a_pattern_are_removed() -> Result<(), String> {
    let frozen = freeze(
        &[path(b"/data/a"), name("cache")],
        &[b"/data/a", b"/data/b", b"/srv/cache/x"],
    )?;
    assert_eq!(frozen.roots, [1]);
    assert!(!frozen.ineffective_rule);
    assert!(freeze(&[path(b"/")], &[b"/data"])?.roots.is_empty());
    assert!(freeze(&[path(b"/data")], &[b"/data/a"])?.roots.is_empty());
    let outside = freeze(&[path(b"/other/x"), name("*.tmp")], &[b"/data"])?;
    assert_eq!(outside.roots, [0]);
    assert!(outside.ineffective_rule);
    // A sibling that only shares a byte prefix is not below the rule.
    assert_eq!(freeze(&[path(b"/data/a")], &[b"/data/ab"])?.roots, [0]);
    // A root name that a pattern cannot decide withholds the whole review.
    assert!(freeze(&[name("?")], &[b"/data/\xff"]).is_err());
    Ok(())
}

#[test]
fn name_patterns_match_raw_names_with_wildcards() -> Result<(), String> {
    let pattern = |text: &str| Pattern::new(text).ok_or(format!("rejected {text}"));
    let tmp = pattern("*.tmp")?;
    assert_eq!(tmp.matches(b"a.tmp"), Some(true));
    assert_eq!(tmp.matches(b".tmp"), Some(true));
    assert_eq!(tmp.matches(b"a.TMP"), Some(false));
    assert_eq!(tmp.matches(b"a.tmp.bak"), Some(false));
    assert_eq!(tmp.matches(b"\xff.tmp"), Some(true));
    assert_eq!(pattern("raw-*")?.matches(b"raw-\xff"), Some(true));
    let one = pattern("?.log")?;
    assert_eq!(one.matches("\u{e9}.log".as_bytes()), Some(true));
    assert_eq!(one.matches(b"ab.log"), Some(false));
    assert_eq!(one.matches(b"\xff.log"), None);
    let star = pattern("*a")?;
    assert_eq!(star.matches(b"*ba"), Some(true));
    assert_eq!(star.matches(b"*b"), Some(false));
    let spans = pattern("a*b*c")?;
    assert_eq!(spans.matches(b"aXbYc"), Some(true));
    assert_eq!(spans.matches(b"abc"), Some(true));
    assert_eq!(spans.matches(b"acb"), Some(false));
    assert_eq!(pattern("back\\slash")?.matches(b"back\\slash"), Some(true));
    assert_eq!(
        pattern("caf\u{e9}")?.matches("cafe\u{301}".as_bytes()),
        Some(false)
    );
    Ok(())
}

#[test]
fn name_patterns_exclude_at_any_depth_and_withhold_uncountable_names() -> Result<(), String> {
    let frozen = freeze(&[name("*.tmp"), name("?.log")], &[b"/data"])?;
    let filter = &frozen.filter;
    assert_eq!(filter(0, &[b"sub".to_vec()], b"a.tmp"), Exclude);
    assert_eq!(filter(0, &[], b"keep"), Admit);
    assert_eq!(filter(0, &[], b"\xff.tmp"), Exclude);
    assert_eq!(filter(0, &[], b"\xff.log"), Undecidable);
    Ok(())
}

#[test]
fn admission_rejects_malformed_and_foreign_rules() {
    let reason = |rule: ScopeExclusion| UnixRules::admit(&[rule]).err();
    for raw in [&b""[..], b"relative/x", b"/data/\0x"] {
        assert_eq!(
            reason(path(raw)),
            Some(EngineError::reason("path.unresolvable")),
            "{raw:?}"
        );
    }
    let wide: Vec<u8> = "C:\\data"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let windows = ScopeExclusion::Path {
        path: OsPath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&wide),
            display: "C:\\data".into(),
            lossy: false,
        },
    };
    let invalid = Some(EngineError::reason("scope.exclude_invalid"));
    for rule in [
        path(b"/data/../x"),
        path(b"/data/./x"),
        path(b"/.."),
        windows,
        name(""),
        name("a/b"),
        name("a\0b"),
    ] {
        assert_eq!(reason(rule.clone()), invalid, "{rule:?}");
    }
    assert!(UnixRules::admit(&[path(b"/"), name("*")]).is_ok());
}

#[test]
fn unix_drafts_need_no_comparison_provider_and_windows_drafts_still_do() {
    let draft = |includes: Vec<OsPath>, exclusions| ScopeDraft {
        includes,
        exclusions,
        boundary: ScopeBoundary::CurrentVolume,
        included_mounts: Vec::new(),
        source: ScopeSource::Gui,
    };
    let rules = vec![path(b"/data/x"), name("*.tmp")];
    assert_eq!(
        check(&draft(vec![unix(b"/data")], rules.clone()), false),
        Ok(())
    );
    let wide: Vec<u8> = "C:\\data"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let windows = OsPath {
        encoding: PathEncoding::WindowsUtf16le,
        raw: PathBytes::from_bytes(&wide),
        display: "C:\\data".into(),
        lossy: false,
    };
    let invalid = Err(EngineError::reason("scope.exclude_invalid"));
    let windows_rule = vec![ScopeExclusion::Path {
        path: windows.clone(),
    }];
    assert_eq!(
        check(&draft(vec![windows.clone()], windows_rule.clone()), false),
        invalid
    );
    assert_eq!(
        check(&draft(vec![windows.clone()], windows_rule), true),
        Ok(())
    );
    assert_eq!(
        check(&draft(vec![windows.clone()], vec![name("*.tmp")]), true),
        invalid
    );
    // Mixed included encodings fall back to the provider, which rejects Unix rules.
    assert_eq!(
        check(&draft(vec![unix(b"/data"), windows], rules), false),
        invalid
    );
}
