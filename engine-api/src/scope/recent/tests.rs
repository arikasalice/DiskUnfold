use super::*;
use du_core_contracts::{
    PathBytes, PathEncoding,
    volume::{RecentVolumeEntry, RecentVolumeObservation},
};

struct Observed(Option<RecentVolumeObservation>);

impl RecentVolumePort for Observed {
    fn recent_volume(
        &self,
        _: &[u8],
        _: &OsPath,
        continue_work: &dyn Fn() -> bool,
    ) -> Option<RecentVolumeObservation> {
        if continue_work() {
            self.0.clone()
        } else {
            None
        }
    }
}

fn path(raw: &[u8]) -> OsPath {
    OsPath {
        encoding: PathEncoding::Unix,
        raw: PathBytes::from_bytes(raw),
        display: "mount".into(),
        lossy: false,
    }
}

#[test]
fn maps_verified_volume_evidence_without_modifying_the_saved_entry() {
    let saved = path(b"/old");
    let proposed = path(b"/new");
    let observe = |entries| {
        Observed(Some(RecentVolumeObservation {
            identity_kind: b"native-id",
            entries,
        }))
    };
    let run = |port: Observed| review_recent_volume(&port, b"id", &saved, &|| true);
    let same = RecentVolumeEntry {
        path: saved.clone(),
        same_entry: true,
    };
    let moved = RecentVolumeEntry {
        path: proposed.clone(),
        same_entry: false,
    };
    assert_eq!(run(observe(vec![same.clone()])), RecentVolumeMatch::Matched);
    assert_eq!(
        run(observe(vec![moved.clone()])),
        RecentVolumeMatch::Moved {
            proposed_entry: proposed
        }
    );
    assert_eq!(run(observe(vec![])), RecentVolumeMatch::Disconnected);
    assert_eq!(
        run(observe(vec![same, moved])),
        RecentVolumeMatch::Unverifiable
    );
    assert_eq!(run(Observed(None)), RecentVolumeMatch::Unverifiable);
    assert_eq!(saved.raw, PathBytes::from_bytes(b"/old"));
}

#[test]
fn opening_a_replaced_volume_cannot_produce_a_review_fingerprint() {
    use du_core_contracts::{
        analysis_ipc::ScopeBoundary,
        scan_observation::{AnchoredDirectory, DirectoryObservations, ObservedEntry},
    };
    struct Empty;
    impl DirectoryObservations for Empty {
        fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
            Ok(Vec::new())
        }
    }
    impl AnchoredDirectory for Empty {
        fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
            Err(PortError {
                kind: PortErrorKind::NotFound,
                reason_key: "path.unresolvable".into(),
                os_code: None,
            })
        }
    }
    struct Roots(OsPath);
    impl ScopeRootPort for Roots {
        fn open_root(&self, _: &[u8]) -> Result<OpenedScopeRoot, PortError> {
            Ok(OpenedScopeRoot {
                canonical_path: self.0.clone(),
                volume_id: b"new-volume".to_vec(),
                file_id: vec![1],
                directory: Box::new(Empty),
            })
        }
    }
    let raw: Vec<_> = r"C:\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut native = path(&raw);
    native.encoding = PathEncoding::WindowsUtf16le;
    let draft = ScopeDraft {
        includes: vec![native.clone()],
        exclusions: vec![],
        boundary: ScopeBoundary::CurrentVolume,
        included_mounts: vec![],
        source: ScopeSource::Recent,
    };
    let roots = Roots(native);
    let review =
        |volume: &[u8]| review_matched_scope(&draft, 1, &[volume.to_vec()], &roots, || 0, &|| true);
    assert!(review(b"new-volume").is_some());
    assert!(review(b"saved-volume").is_none());
}
