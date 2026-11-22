//! The share of the scope no root sibling can take as area (F06 README 51.3), and the
//! table row the summary line opens.
//!
//! The share is the set of measured bytes under siblings without an area that no
//! attributed sibling holds: exact, unless an unread entry may lie under one of them.
use super::lower_bound::{Entry, is_lower_bound, project_at, row_of, scan};
use super::*;
use du_analysis_index::visualization::projection::UnknownShare;

#[test]
fn a_file_shared_by_two_siblings_is_counted_once_without_an_unknown_share() {
    // a and c hold the same 10-byte file (two links); b holds 20 bytes of its own.
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::Dir("b"),
            Entry::Dir("c"),
            Entry::Linked("a/fa", 10, 7, 2),
            Entry::Linked("c/fc", 10, 7, 2),
            Entry::File("b/fb", 20),
        ],
        ResultCompleteness::Complete,
        &[],
    );
    let model = project_at(&version, "", SizeBasis::Allocated, 16).unwrap();
    assert_eq!(model.detail.unknown_items, 0);
    assert_eq!(model.detail.unknown_share, UnknownShare::None);
    assert_eq!(model.detail.first_unknown_row, None);
    assert_eq!(model.shapes.len(), 2);
    assert_eq!(
        version.shared_counted_item(&find(&version, "fa")).unwrap(),
        Some(find(&version, "fa"))
    );
}

#[test]
fn an_unread_entry_under_an_attributed_sibling_does_not_make_a_shared_file_unknown() {
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::Dir("b"),
            Entry::Dir("c"),
            Entry::Linked("a/fa", 10, 7, 2),
            Entry::Linked("c/fc", 10, 7, 2),
            Entry::File("b/fb", 20),
            Entry::Unread("b/locked"),
        ],
        ResultCompleteness::Missing,
        &["b/locked"],
    );
    let model = project_at(&version, "", SizeBasis::Allocated, 16).unwrap();
    assert!(matches!(
        model.totals.scope.amount,
        SizeAmount::LowerBound { .. }
    ));
    assert_eq!(row_of(&model, &version, "b").omission, None);
    assert_eq!(model.detail.unknown_items, 0);
    assert_eq!(model.detail.unknown_share, UnknownShare::None);
    assert_eq!(model.detail.first_unknown_row, None);
}

#[test]
fn an_unread_entry_under_a_counted_sibling_does_not_create_an_unknown_share() {
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::Dir("b"),
            Entry::Dir("c"),
            Entry::Linked("a/fa", 10, 7, 2),
            Entry::Unread("a/locked"),
            Entry::Linked("c/fc", 10, 7, 2),
            Entry::File("b/fb", 20),
        ],
        ResultCompleteness::Missing,
        &["a/locked"],
    );
    let model = project_at(&version, "", SizeBasis::Allocated, 16).unwrap();
    assert!(is_lower_bound(&row_of(&model, &version, "a").size));
    assert_eq!(model.detail.unknown_items, 0);
    assert_eq!(model.detail.unknown_share, UnknownShare::None);
}

#[test]
fn an_unread_entry_is_a_zero_lower_bound_and_no_item_without_an_area() {
    // Since F03 #344 an unread entry is a known lower bound of zero: it has no area
    // and is not an item without a provable area, so it leaves no share and no row.
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::File("a/fa", 100),
            Entry::Unread("locked"),
        ],
        ResultCompleteness::Missing,
        &["locked"],
    );
    let model = project_at(&version, "", SizeBasis::Logical, 16).unwrap();
    assert_eq!(model.detail.unknown_items, 0);
    assert_eq!(model.detail.unknown_share, UnknownShare::None);
    assert_eq!(model.detail.first_unknown_row, None);
}

#[test]
fn the_table_position_counts_the_group_row_instead_of_its_members() {
    // Many small files become the root group; unread siblings stay without an area.
    let mut entries = vec![
        Entry::Dir("a"),
        Entry::Dir("c"),
        Entry::Unread("a/unread"),
        Entry::Unread("c/unread"),
        Entry::File("big", 1_000),
    ];
    let names: Vec<String> = (0..120).map(|i| format!("s{i:03}")).collect();
    // Each is below the minimum area; together they are well above it.
    entries.extend(names.iter().map(|name| Entry::File(name, 10)));
    let version = scan(&entries, ResultCompleteness::Complete, &[]);
    let model = project_at(&version, "", SizeBasis::Allocated, 16).unwrap();
    let group = model.detail.groups.first().unwrap();
    assert_eq!(group.parent, model.parent);
    let first = model.detail.first_unknown_row.unwrap();
    let kept = model
        .rows
        .iter()
        .filter(|row| !matches!(row.lod, Lod::Grouped(_)))
        .count();
    // The pair comes after every kept row, and the group row stands among them.
    assert_eq!(model.table_position(first), Some(kept + 1 - 2));
    assert_eq!(model.detail.unknown_items, 2);
    assert_eq!(model.detail.unknown_share, UnknownShare::None);
}
