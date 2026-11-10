use super::*;

#[test]
fn same_identity_at_same_or_new_entry_is_not_applied_to_saved_scope() {
    let saved = Identity::Confirmed {
        kind: b"guid",
        value: b"id-a",
    };
    let same = CurrentVolume {
        identity: saved,
        entry_path: b"C:",
        entry_comparison: EntryComparison::Same,
    };
    assert_eq!(
        correspond(saved, Inventory::Complete, &[same]),
        Correspondence::Matched
    );
    assert_eq!(
        correspond(saved, Inventory::Partial, &[same]),
        Correspondence::Matched
    );
    let moved = CurrentVolume {
        entry_path: b"D:",
        entry_comparison: EntryComparison::Different,
        ..same
    };
    assert_eq!(
        correspond(saved, Inventory::Complete, &[moved]),
        Correspondence::Moved {
            proposed_entry: b"D:"
        }
    );
    assert_eq!(
        correspond(saved, Inventory::Partial, &[moved]),
        Correspondence::Unverifiable
    );
}

#[test]
fn same_name_different_identity_never_matches_and_absence_needs_complete_inventory() {
    let saved = Identity::Confirmed {
        kind: b"guid",
        value: b"id-a",
    };
    let other = CurrentVolume {
        identity: Identity::Confirmed {
            kind: b"guid",
            value: b"id-b",
        },
        entry_path: b"C:",
        entry_comparison: EntryComparison::Same,
    };
    assert_eq!(
        correspond(saved, Inventory::Complete, &[other]),
        Correspondence::Disconnected
    );
    assert_eq!(
        correspond(saved, Inventory::Partial, &[other]),
        Correspondence::Unverifiable
    );
    let different_kind = CurrentVolume {
        identity: Identity::Confirmed {
            kind: b"other",
            value: b"id-a",
        },
        ..other
    };
    assert_eq!(
        correspond(saved, Inventory::Complete, &[different_kind]),
        Correspondence::Unverifiable
    );
    assert_eq!(
        correspond(saved, Inventory::Unavailable, &[]),
        Correspondence::Unverifiable
    );
    assert_eq!(
        correspond(Identity::Unknown, Inventory::Complete, &[other]),
        Correspondence::Unverifiable
    );
    assert_eq!(
        correspond(
            Identity::Confirmed {
                kind: b"guid",
                value: b""
            },
            Inventory::Complete,
            &[]
        ),
        Correspondence::Unverifiable
    );
}

#[test]
fn ambiguous_unknown_or_unverified_entries_never_produce_a_proposal() {
    let saved = Identity::Confirmed {
        kind: b"guid",
        value: b"id-a",
    };
    let a = CurrentVolume {
        identity: saved,
        entry_path: b"C:",
        entry_comparison: EntryComparison::Same,
    };
    let b = CurrentVolume {
        entry_path: b"D:",
        entry_comparison: EntryComparison::Different,
        ..a
    };
    assert_eq!(
        correspond(saved, Inventory::Complete, &[a, b]),
        Correspondence::Unverifiable
    );
    let unknown = CurrentVolume {
        identity: Identity::Unknown,
        ..b
    };
    assert_eq!(
        correspond(saved, Inventory::Complete, &[a, unknown]),
        Correspondence::Unverifiable
    );
    assert_eq!(
        correspond(saved, Inventory::Complete, &[unknown]),
        Correspondence::Unverifiable
    );
    assert_eq!(
        correspond(
            saved,
            Inventory::Complete,
            &[CurrentVolume {
                entry_comparison: EntryComparison::Unknown,
                ..a
            }]
        ),
        Correspondence::Unverifiable
    );
    assert_eq!(
        correspond(
            saved,
            Inventory::Complete,
            &[CurrentVolume {
                entry_path: b"",
                ..a
            }]
        ),
        Correspondence::Unverifiable
    );
}
