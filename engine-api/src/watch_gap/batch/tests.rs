#![allow(clippy::unwrap_used)]
use super::*;
use crate::watch_gap::{Ledger, Outcome};
use du_scan_engine::{
    watch_coalescer::{Batch, Change, Context, Gap, GapReason, Trust},
    watch_lifecycle::{Definition, Snapshot, State},
};
use std::collections::BTreeSet;

struct Owner;
impl ScopeOwner for Owner {
    type Target = u8;
    type Component = Vec<u8>;
    fn admit(
        &mut self,
        _: [u8; 16],
        watch: [u8; 16],
        path: Option<&Key>,
    ) -> Option<Admitted<u8, Vec<u8>>> {
        let components = path.cloned().unwrap_or_default();
        if let Some(b"access" | b"boundary") = components.first().map(Vec::as_slice) {
            return None;
        }
        let volume = if path.is_none() || components.first().is_some_and(|c| c == b"volume") {
            Volume::Unknown
        } else {
            Volume::Matching
        };
        Some(Admitted {
            target: watch[0],
            components,
            base: BaseResult::Compatible,
            volume,
        })
    }
}
fn definition(scope: u8) -> Definition {
    Definition {
        watch_ref: [3; 16],
        scope_revision: [scope; 16],
        target: vec![],
        entry_path: vec![1],
        volume_path: vec![],
        nested_mounts: vec![],
        volume: vec![2],
        baseline_end: Some(0),
    }
}
fn change(path: &[&str], subtree: bool) -> Change {
    Change {
        path: path.iter().map(|x| x.as_bytes().to_vec()).collect(),
        subtree,
        kinds: BTreeSet::new(),
        first: 1,
        last: 1,
    }
}
fn exercise(scope: u8) -> Plan<u8, Vec<u8>> {
    let definition = definition(scope);
    let mut ledger = Ledger::new([scope; 16]);
    ledger.register::<()>(definition.clone()).unwrap();
    let snapshot = Snapshot {
        definition,
        state: State::Active,
        generation: 1,
        sequence: 0,
        last_change_sequence: 0,
        last_observed: Some(0),
        observed_at: 0,
        gaps: vec![],
    };
    ledger
        .apply_snapshot(&snapshot, &mut |_| Ok::<(), ()>(()))
        .unwrap();
    let batch = Batch {
        context: Context {
            watch_ref: [3; 16],
            scope_revision: [scope; 16],
            volume: vec![2],
            generation: 1,
        },
        sequence: 1,
        change_sequence: 1,
        replaced_gap_ids: vec![],
        interval_start: 1,
        interval_end: 1,
        changes: vec![
            change(&["a"], true),
            change(&["a", "child"], false),
            change(&["access"], true),
            change(&["boundary"], true),
            change(&["volume"], true),
        ],
        gaps: vec![Gap {
            id: 1,
            reason: GapReason::Overflow,
            path: None,
            start: Some(0),
            end: Some(1),
        }],
        trust: Trust::Gap,
    };
    let mut result = None;
    assert_eq!(
        ledger
            .apply(&batch, &mut |update| {
                result = prepare([scope; 16], update, &mut Owner, 256);
                Ok::<(), ()>(())
            })
            .unwrap(),
        Outcome::Applied
    );
    result.unwrap()
}
#[test]
fn consumer_callback_separates_auto_and_recovery_and_fails_closed() {
    let p = exercise(1);
    assert_eq!(
        p.automatic.get(&3),
        Some(&Selection::Directories(vec![Directory {
            components: vec![b"a".to_vec()],
            include_descendants: true
        }]))
    );
    assert_eq!(p.recovery, vec![(3, RecoveryReason::GapUnlocatable)]);
    assert_ne!(p.scope, exercise(2).scope);
}

#[test]
fn closed_directory_gap_is_automatic_but_open_gap_is_not() {
    let mut gaps = BTreeMap::new();
    gaps.insert(
        1,
        Gap {
            id: 1,
            reason: GapReason::SignalLost,
            path: Some(vec![b"closed".to_vec()]),
            start: Some(0),
            end: Some(1),
        },
    );
    gaps.insert(
        2,
        Gap {
            id: 2,
            reason: GapReason::SignalLost,
            path: Some(vec![b"open".to_vec()]),
            start: Some(0),
            end: None,
        },
    );
    let batch = Batch {
        context: Context {
            watch_ref: [3; 16],
            scope_revision: [1; 16],
            volume: vec![2],
            generation: 1,
        },
        sequence: 1,
        change_sequence: 1,
        replaced_gap_ids: vec![],
        interval_start: 1,
        interval_end: 1,
        changes: vec![],
        gaps: gaps.values().cloned().collect(),
        trust: Trust::Gap,
    };
    let previous_gaps = BTreeMap::new();
    let p = prepare(
        [1; 16],
        Update::Batch {
            batch: &batch,
            previous_gaps: &previous_gaps,
            gaps: &gaps,
        },
        &mut Owner,
        256,
    )
    .unwrap();
    assert_eq!(
        p.automatic.get(&3),
        Some(&Selection::Directories(vec![Directory {
            components: vec![b"closed".to_vec()],
            include_descendants: true
        }]))
    );
    assert!(p.recovery.is_empty());
    assert!(
        prepare(
            [2; 16],
            Update::Batch {
                batch: &batch,
                previous_gaps: &previous_gaps,
                gaps: &gaps
            },
            &mut Owner,
            256
        )
        .is_none()
    );
}

mod transitions;
