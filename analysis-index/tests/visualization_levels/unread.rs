//! An entry that could not be read, and the scopes around an unread range.
//!
//! The entry has no value, so it is counted without an area and the value above it is a
//! lower bound; a scope inside an unread range cannot be partitioned at all.
use super::lower_bound::{Entry, drawn_has_a_lower_bound, is_lower_bound, project_at, scan};
use super::*;

#[test]
fn an_unread_entry_leaves_a_lower_bound_and_the_gap_reason_in_the_summary() {
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::File("a/fa", 100),
            Entry::File("top", 50),
            Entry::Unread("locked"),
        ],
        ResultCompleteness::Missing,
        &["locked"],
    );
    let model = project_at(&version, "", SizeBasis::Logical, 16).unwrap();
    // The unread entry is at least zero bytes (#341), so it has no area and is not an
    // unknown item; the rest is drawn by its own values.
    assert_eq!(model.shapes.len(), 2);
    assert_eq!(model.detail.unknown_items, 0);
    assert_eq!(model.detail.reason_keys, ["scan.path_denied"]);
    // The unread entry is directly under the scope, so the scope is a lower bound.
    assert!(is_lower_bound(&model.totals.scope));
    assert_eq!(model.totals.drawn_bytes, 150);
    // Whether a folder that was read completely is a lower bound depends on how F03
    // treats a result with unread ranges (the whole result today; only the folders
    // above an unread range once FR5.HD1 is applied per folder). This test holds for
    // both, so it checks that the state of each total follows the rows it adds.
    assert_eq!(
        model.totals.drawn_lower_bound,
        drawn_has_a_lower_bound(&model)
    );
    let folder = project_at(&version, "a", SizeBasis::Logical, 16).unwrap();
    assert_eq!(folder.shapes.len(), 1);
    assert_eq!(
        folder.totals.drawn_lower_bound,
        drawn_has_a_lower_bound(&folder)
    );
    assert!(folder.detail.reason_keys.is_empty());
}

#[test]
fn a_scope_inside_an_unread_range_is_still_refused() {
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::File("a/fa", 100),
            Entry::File("top", 50),
        ],
        ResultCompleteness::Missing,
        &["a"],
    );
    // The range at a covers a, so nothing inside it can be partitioned; the scope above
    // it can, and reports the range.
    assert_eq!(
        project_at(&version, "a", SizeBasis::Logical, 16).err(),
        Some(projection::Error::UnavailableAttribution)
    );
    let root = project_at(&version, "", SizeBasis::Logical, 16).unwrap();
    assert_eq!(root.detail.reason_keys, ["scan.path_denied"]);
}

#[test]
fn unique_physical_size_keeps_its_exact_only_rule() {
    // An entry that could not be read keeps the value above it a lower bound or unknown
    // on every basis; unique physical size takes no lower bound, so it is refused, and
    // the logical size of the same scope is drawn.
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::File("a/fa", 100),
            Entry::Unread("locked"),
        ],
        ResultCompleteness::Complete,
        &[],
    );
    assert_eq!(
        project_at(&version, "", SizeBasis::UniquePhysical, 16).err(),
        Some(projection::Error::UnavailableAttribution)
    );
    let logical = project_at(&version, "", SizeBasis::Logical, 16).unwrap();
    assert_eq!(logical.shapes.len(), 1);
    assert_eq!(logical.detail.unknown_items, 1);
}
