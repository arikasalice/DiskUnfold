use du_core_contracts::{
    OsPath, PathBytes, PathEncoding,
    analysis_ipc::{ScopeBoundary, ScopeDraft, ScopeExclusion, ScopeSource},
    ports::{PortError, PortErrorKind},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, ObservedEntry, OpenedScopeRoot, ScopeRootPort,
    },
};
use du_engine_api::scope::preparation::{self, Error, RootEvidence};
use du_scan_engine::scope::{Notice, Path};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn path(text: &str) -> OsPath {
    let raw: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    OsPath {
        encoding: PathEncoding::WindowsUtf16le,
        raw: PathBytes::from_bytes(&raw),
        display: text.into(),
        lossy: false,
    }
}
fn raw(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
fn failure() -> PortError {
    PortError {
        kind: PortErrorKind::NotFound,
        reason_key: "test.missing".into(),
        os_code: None,
    }
}

struct Handle {
    live: Arc<AtomicUsize>,
    ancestors: Option<Vec<Vec<u8>>>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}
impl DirectoryObservations for Handle {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(Vec::new())
    }
}
impl AnchoredDirectory for Handle {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(failure())
    }
    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        self.ancestors.as_deref()
    }
}

type Ancestors = BTreeMap<Vec<u8>, Option<Vec<Vec<u8>>>>;

struct Fake {
    files: Mutex<BTreeMap<Vec<u8>, (u8, u8)>>,
    facts: Mutex<BTreeMap<Vec<u8>, Path>>,
    ancestors: Mutex<Ancestors>,
    open_count: AtomicUsize,
    live: Arc<AtomicUsize>,
}
impl ScopeRootPort for Fake {
    fn open_root(&self, input: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        self.open_count.fetch_add(1, Ordering::SeqCst);
        let (volume, file) = *self
            .files
            .lock()
            .map_err(|_| failure())?
            .get(input)
            .ok_or_else(failure)?;
        self.live.fetch_add(1, Ordering::SeqCst);
        Ok(OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(input),
                display: "ignored".into(),
                lossy: false,
            },
            volume_id: vec![volume],
            file_id: vec![file],
            directory: Box::new(Handle {
                live: self.live.clone(),
                ancestors: self
                    .ancestors
                    .lock()
                    .map_err(|_| failure())?
                    .get(input)
                    .cloned()
                    .flatten(),
            }),
        })
    }
}
impl RootEvidence for Fake {
    fn observe(&self, root: &OpenedScopeRoot) -> Result<Path, Error> {
        let bytes = root
            .canonical_path
            .raw
            .bytes()
            .map_err(|_| Error::InvalidPath)?;
        self.facts
            .lock()
            .map_err(|_| Error::Unavailable)?
            .get(&bytes)
            .cloned()
            .ok_or(Error::Unavailable)
    }
}
fn fake() -> Fake {
    let mut files = BTreeMap::new();
    let mut facts = BTreeMap::new();
    for (name, volume, file, keys) in [
        ("C:\\a", 1, 11, vec!["a"]),
        ("C:\\a\\b", 1, 12, vec!["a", "b"]),
        ("D:\\z", 2, 21, vec!["z"]),
    ] {
        let raw = raw(name);
        files.insert(raw.clone(), (volume, file));
        facts.insert(
            raw,
            Path {
                volume: Some(vec![volume]),
                entry: vec![volume],
                alias_verified: true,
                boundary_verified: true,
                comparison_verified: true,
                key: keys
                    .into_iter()
                    .map(|key| key.as_bytes().to_vec())
                    .collect(),
            },
        );
    }
    Fake {
        files: Mutex::new(files),
        facts: Mutex::new(facts),
        ancestors: Mutex::new(BTreeMap::from([
            (raw("C:\\a"), Some(vec![vec![10]])),
            (raw("C:\\a\\b"), Some(vec![vec![10], vec![11]])),
            (raw("D:\\z"), Some(vec![vec![20]])),
        ])),
        open_count: AtomicUsize::new(0),
        live: Arc::new(AtomicUsize::new(0)),
    }
}
fn draft(includes: &[&str]) -> ScopeDraft {
    ScopeDraft {
        includes: includes.iter().map(|p| path(p)).collect(),
        exclusions: vec![],
        boundary: ScopeBoundary::CurrentVolume,
        included_mounts: vec![],
        source: ScopeSource::Gui,
    }
}

fn bindings(prepared: &preparation::Prepared) -> Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    prepared
        .selected_roots()
        .map(|root| {
            (
                root.canonical_path.raw.as_str().as_bytes().to_vec(),
                root.volume_id.to_vec(),
                root.file_id.to_vec(),
            )
        })
        .collect()
}

fn checked_start(
    draft: &ScopeDraft,
    fake: &Fake,
    expected: &[(Vec<u8>, Vec<u8>, Vec<u8>)],
) -> Result<Vec<OpenedScopeRoot>, Error> {
    let prepared = preparation::prepare(draft, fake, fake)?;
    if bindings(&prepared) != expected {
        return Err(Error::InvalidEvidence);
    }
    Ok(prepared.into_roots())
}

#[test]
fn multiple_roots_are_ordered_and_only_confirmed_overlap_is_merged() -> Result<(), String> {
    let fake = fake();
    let first = preparation::prepare(
        &draft(&["D:\\z", "C:\\a\\b", "C:\\a", "C:\\a"]),
        &fake,
        &fake,
    )
    .map_err(|e| format!("{e:?}"))?;
    let second = preparation::prepare(
        &draft(&["C:\\a", "C:\\a", "D:\\z", "C:\\a\\b"]),
        &fake,
        &fake,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(bindings(&first), bindings(&second));
    assert_eq!(first.normalized_roots().count(), 2);
    assert_eq!(first.selected_roots().count(), 4);
    assert_eq!(first.normalized_roots().count(), 2);
    assert!(
        first
            .notices()
            .iter()
            .any(|n| matches!(n, Notice::Duplicate(_)))
    );
    assert!(
        first
            .notices()
            .iter()
            .any(|n| matches!(n, Notice::DescendantAbsorbed(_)))
    );
    assert_eq!(fake.open_count.load(Ordering::SeqCst), 8);
    drop(first);
    assert_eq!(fake.live.load(Ordering::SeqCst), 4);
    drop(second);
    assert_eq!(fake.live.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn replaced_absorbed_root_and_missing_root_fail_closed_without_leaks() -> Result<(), String> {
    let fake = fake();
    let input = draft(&["C:\\a", "C:\\a\\b", "D:\\z"]);
    let review = preparation::prepare(&input, &fake, &fake).map_err(|e| format!("{e:?}"))?;
    let expected = bindings(&review);
    drop(review);
    fake.files
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .insert(raw("C:\\a\\b"), (1, 99));
    assert!(matches!(
        checked_start(&input, &fake, &expected),
        Err(Error::InvalidEvidence)
    ));
    assert_eq!(fake.live.load(Ordering::SeqCst), 0);
    fake.files
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .remove(&raw("D:\\z"));
    assert!(matches!(
        checked_start(&input, &fake, &expected),
        Err(Error::Unavailable)
    ));
    assert_eq!(fake.live.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn uncertain_relationships_stay_separate_and_unsupported_scope_is_rejected() -> Result<(), String> {
    let fake = fake();
    fake.facts
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .get_mut(&raw("C:\\a\\b"))
        .ok_or("missing facts")?
        .boundary_verified = false;
    fake.ancestors
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .insert(raw("C:\\a\\b"), None);
    let review = preparation::prepare(&draft(&["C:\\a", "C:\\a\\b"]), &fake, &fake)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(review.normalized_roots().count(), 2);
    assert!(review.notices().contains(&Notice::BoundaryUncertain));
    let mut bad = draft(&["C:\\a"]);
    bad.exclusions.push(ScopeExclusion::Path {
        path: path("C:\\a\\b"),
    });
    assert!(matches!(
        preparation::prepare(&bad, &fake, &fake),
        Err(Error::UnsupportedScope)
    ));
    assert!(matches!(
        preparation::prepare(&draft(&[]), &fake, &fake),
        Err(Error::Empty)
    ));
    Ok(())
}

#[test]
fn preparation_exposes_current_comparison_facts_without_a_second_open() -> Result<(), String> {
    let fake = fake();
    fake.facts
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .get_mut(&raw("D:\\z"))
        .ok_or("missing facts")?
        .comparison_verified = false;
    let prepared = preparation::prepare(&draft(&["C:\\a", "D:\\z"]), &fake, &fake)
        .map_err(|e| format!("{e:?}"))?;
    let roots: Vec<_> = prepared.normalized_roots().collect();
    assert_eq!(roots.len(), 2);
    assert!(
        roots
            .iter()
            .any(|root| !root.comparison_verified && root.comparison_components == [b"z".to_vec()])
    );
    assert!(
        roots
            .iter()
            .all(|root| root.canonical_path.encoding == PathEncoding::WindowsUtf16le)
    );
    assert_eq!(fake.open_count.load(Ordering::SeqCst), 2);
    let handles = prepared.into_roots();
    assert_eq!(handles.len(), 2);
    assert_eq!(fake.live.load(Ordering::SeqCst), 2);
    drop(handles);
    assert_eq!(fake.live.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn equal_comparison_keys_with_conflicting_identity_are_never_merged() -> Result<(), String> {
    let fake = fake();
    let mut facts = fake.facts.lock().map_err(|e| format!("{e:?}"))?;
    let mut same = facts.get(&raw("C:\\a")).cloned().ok_or("missing facts")?;
    same.alias_verified = false;
    facts.insert(raw("D:\\z"), same);
    drop(facts);
    fake.files
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .insert(raw("D:\\z"), (1, 99));
    assert!(matches!(
        preparation::prepare(&draft(&["C:\\a", "D:\\z"]), &fake, &fake),
        Err(Error::ConflictingIdentity)
    ));
    assert_eq!(fake.live.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn path_prefix_without_anchored_ancestor_never_merges() -> Result<(), String> {
    let fake = fake();
    fake.ancestors
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .insert(raw("C:\\a\\b"), None);
    let prepared = preparation::prepare(&draft(&["C:\\a", "C:\\a\\b"]), &fake, &fake)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(prepared.normalized_roots().count(), 2);
    assert!(prepared.notices().contains(&Notice::BoundaryUncertain));
    Ok(())
}

#[test]
fn same_identity_merges_aliases_without_comparison_rules() -> Result<(), String> {
    let fake = fake();
    fake.files
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .insert(raw("D:\\z"), (1, 11));
    let mut facts = fake.facts.lock().map_err(|e| format!("{e:?}"))?;
    let alias = facts.get_mut(&raw("D:\\z")).ok_or("missing facts")?;
    alias.volume = Some(vec![1]);
    alias.comparison_verified = false;
    alias.alias_verified = false;
    drop(facts);
    let prepared = preparation::prepare(&draft(&["D:\\z", "C:\\a"]), &fake, &fake)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(prepared.normalized_roots().count(), 1);
    assert_eq!(prepared.selected_roots().count(), 2);
    assert!(
        prepared
            .notices()
            .iter()
            .any(|notice| matches!(notice, Notice::AliasMerged(_)))
    );
    assert_eq!(fake.live.load(Ordering::SeqCst), 2);
    let roots = prepared.into_roots();
    assert_eq!(roots.len(), 1);
    assert_eq!(fake.live.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn contradictory_ancestor_cycle_cannot_hide_roots_behind_an_unrelated_selection()
-> Result<(), String> {
    let fake = fake();
    let mut ancestors = fake.ancestors.lock().map_err(|e| format!("{e:?}"))?;
    ancestors.insert(raw("C:\\a"), Some(vec![vec![12]]));
    ancestors.insert(raw("C:\\a\\b"), Some(vec![vec![11]]));
    drop(ancestors);
    let result = preparation::prepare(&draft(&["C:\\a", "C:\\a\\b", "D:\\z"]), &fake, &fake);
    assert!(matches!(result, Err(Error::ConflictingIdentity)));
    assert_eq!(fake.live.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn contradictory_cycle_is_rejected_even_when_both_name_a_retained_ancestor() -> Result<(), String> {
    let fake = fake();
    fake.files
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .insert(raw("D:\\z"), (1, 21));
    fake.facts
        .lock()
        .map_err(|e| format!("{e:?}"))?
        .get_mut(&raw("D:\\z"))
        .ok_or("missing facts")?
        .volume = Some(vec![1]);
    let mut ancestors = fake.ancestors.lock().map_err(|e| format!("{e:?}"))?;
    ancestors.insert(raw("C:\\a"), Some(vec![vec![12], vec![21]]));
    ancestors.insert(raw("C:\\a\\b"), Some(vec![vec![11], vec![21]]));
    ancestors.insert(raw("D:\\z"), Some(vec![]));
    drop(ancestors);
    let result = preparation::prepare(&draft(&["C:\\a", "C:\\a\\b", "D:\\z"]), &fake, &fake);
    assert!(matches!(result, Err(Error::ConflictingIdentity)));
    assert_eq!(fake.live.load(Ordering::SeqCst), 0);
    Ok(())
}
