//! The share of the scope no root sibling can take as area (F06 README 51.3), and the
//! table row the summary line opens.
//!
//! The share is the set of measured bytes under siblings without an area that no
//! attributed sibling holds: exact, unless an unread entry may lie under one of them.
use super::lower_bound::{Entry, is_lower_bound, project_at, row_of, scan};
use super::*;
use du_analysis_index::visualization::projection::UnknownShare;

fn table_row(model: &Model, version: &ResultVersion, name: &str) -> Option<usize> {
    let wanted = find(version, name);
    let index = model.rows.iter().position(|row| row.item == wanted)?;
    model.table_position(index)
}

#[test]
fn a_file_shared_by_two_siblings_is_in_the_share_once() {
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
    assert_eq!(model.detail.unknown_items, 2);
    assert_eq!(model.detail.unknown_share, UnknownShare::Exact(10));
    // b fills the chart; the first row without an area is a, after b in the table.
    assert_eq!(model.shapes.len(), 1);
    let first = model.detail.first_unknown_row.unwrap();
    assert_eq!(model.rows[first].item, find(&version, "a"));
    assert_eq!(
        model.table_position(first),
        table_row(&model, &version, "a")
    );
    assert_eq!(model.table_position(first), Some(1));
}

#[test]
fn an_unread_entry_under_an_attributed_sibling_follows_the_unattributed_values() {
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
    // The share is exact when F03 keeps the siblings without an area exact; whether a
    // result with an unread range makes every folder a lower bound is F03's rule.
    let unattributed_lower = ["a", "c"]
        .iter()
        .any(|name| is_lower_bound(&row_of(&model, &version, name).size));
    let expected = if unattributed_lower {
        UnknownShare::LowerBound(10)
    } else {
        UnknownShare::Exact(10)
    };
    assert_eq!(model.detail.unknown_share, expected);
}

#[test]
fn an_unread_entry_under_a_sibling_without_an_area_makes_the_share_a_lower_bound() {
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
    assert_eq!(
        row_of(&model, &version, "a").omission,
        Some(NoArea::Unknown)
    );
    assert_eq!(model.detail.unknown_share, UnknownShare::LowerBound(10));
}

#[test]
fn an_entry_without_bytes_leaves_no_share_but_is_still_the_first_row() {
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
    assert_eq!(model.detail.unknown_items, 1);
    assert_eq!(model.detail.unknown_share, UnknownShare::None);
    assert_eq!(
        model.detail.first_unknown_row,
        model
            .rows
            .iter()
            .position(|row| row.omission == Some(NoArea::Unknown))
    );
}

#[test]
fn the_table_position_counts_the_group_row_instead_of_its_members() {
    // Many small files become the root group; the shared pair stays without an area.
    let mut entries = vec![
        Entry::Dir("a"),
        Entry::Dir("c"),
        Entry::Linked("a/fa", 10, 7, 2),
        Entry::Linked("c/fc", 10, 7, 2),
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
    assert_eq!(model.detail.unknown_share, UnknownShare::Exact(10));
}
