use super::*;

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
    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        self.inner.observed_identity()
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
        recent_observations: Arc::new(AtomicUsize::new(0)),
        recent_opens: Arc::new(AtomicUsize::new(0)),
        recent_live_handles: Arc::new(AtomicUsize::new(0)),
        recent_unavailable: std::sync::atomic::AtomicBool::new(false),
        read_handles: Arc::new(Mutex::new(Vec::new())),
        release: Arc::new(Mutex::new(receiver)),
        finish_gate: None,
        priority: None,
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
        false,
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(verified.roots.len(), 1);
    let hashed = verified
        .scope_fingerprint
        .retained_comparison_input()
        .ok_or("missing comparison inputs")?;
    assert_ne!(hashed.exclusions, verified.normalized_draft.exclusions);
    let fingerprint =
        crate::scope::recent_scope_fingerprint_v1(&hashed).map_err(|error| format!("{error:?}"))?;
    let applied = crate::scope::recent_scope_fingerprint_v1(&verified.normalized_draft)
        .map_err(|error| format!("{error:?}"))?;
    assert_ne!(fingerprint, applied);
    assert_eq!(
        blake3::Hash::from_bytes(fingerprint).to_hex().as_str(),
        verified
            .scope_fingerprint
            .resolve()
            .map_err(|error| format!("{error:?}"))?
    );
    assert_eq!(
        serde_json::to_value(&hashed.exclusions[0])?["path"]["raw"],
        native("c:\\fixture\\nested\\private")["raw"]
    );
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
