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
