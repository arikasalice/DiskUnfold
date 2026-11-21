//! Siblings whose F03 value is a lower bound are drawn by that bound, on any result.
//!
//! An untraversed link without its own size or an entry that could not be read adds no
//! bytes, so every value above it is a lower bound (F03 FR5.HD1). HLD table 22 draws such a sibling with an
//! area proportional to its lower bound and marks the totals as lower bounds. What has
//! no value of its own, and what the F03 value cannot be matched to, is still counted
//! without an area.
pub(super) use super::scan_fixture::{Entry, scan};
use super::*;
use du_analysis_index::visualization::projection::{Row, add_child_partition};

pub(super) fn project_at(
    version: &ResultVersion,
    folder: &str,
    basis: SizeBasis,
    max_shapes: usize,
) -> Result<Model, projection::Error> {
    let parent = if folder.is_empty() {
        item(version, 0)
    } else {
        find(version, folder)
    };
    projection::project(
        Source {
            version,
            parent: &parent,
            basis,
            bounds: WINDOW,
        },
        &dispatcher(),
        Options {
            format: "treemap",
            strategy: None,
            max_shapes,
            group_tail: true,
        },
        &mut || false,
    )
}

pub(super) fn is_lower_bound(size: &SizeValue) -> bool {
    matches!(size.amount, SizeAmount::LowerBound { .. })
}

/// Whether a row that has a shape of its own carries a lower-bound value.
pub(super) fn drawn_has_a_lower_bound(model: &Model) -> bool {
    model
        .shapes
        .iter()
        .any(|shape| is_lower_bound(&model.rows[shape.row_index].size))
}

pub(super) fn row_of<'a>(model: &'a Model, version: &ResultVersion, name: &str) -> &'a Row {
    let wanted = find(version, name);
    model.rows.iter().find(|row| row.item == wanted).unwrap()
}

fn share_of(model: &Model, version: &ResultVersion, name: &str) -> f64 {
    let wanted = find(version, name);
    let shape = model
        .shapes
        .iter()
        .find(|shape| model.rows[shape.row_index].item == wanted)
        .unwrap();
    area(&shape.geometry) as f64 / 1e12
}

fn one_folder_with_a_link() -> Vec<Entry<'static>> {
    vec![
        Entry::Dir("a"),
        Entry::Dir("b"),
        Entry::Dir("x"),
        Entry::File("a/fa", 100),
        Entry::File("b/fb", 50),
        Entry::File("x/fx", 20),
        Entry::Link("x/lx"),
    ]
}

fn link_two_levels_down() -> Vec<Entry<'static>> {
    vec![
        Entry::Dir("c"),
        Entry::Dir("deep"),
        Entry::Dir("deep/inner"),
        Entry::File("c/f", 100),
        Entry::File("deep/d", 40),
        Entry::File("deep/inner/i", 30),
        Entry::Link("deep/inner/l"),
    ]
}

/// The F03 value of an item on a basis.
pub(super) fn value_of(version: &ResultVersion, name: &str, basis: &SizeBasis) -> SizeValue {
    let item = if name.is_empty() {
        0
    } else {
        find(version, name).item_no
    };
    let space = version.space(item).unwrap();
    match basis {
        SizeBasis::Logical => space.logical.clone(),
        SizeBasis::Allocated => space.allocated.clone(),
        SizeBasis::UniquePhysical => space.unique_physical.clone(),
    }
}

pub(super) fn has_no_value(value: &SizeValue) -> bool {
    value.amount == SizeAmount::Unknown
}

// How F03 values what a link touches is being revised (FR5.HD1). Today a folder above a
// link is a lower bound and the link beside its siblings has no value; once a link is
// counted without lowering anything, the folder is exact and the link is an exact zero.
// Each test below reads the value from F03 and checks what the treemap does with it, so
// it holds for both.

#[test]
fn a_complete_result_draws_a_folder_with_a_link_by_its_value_on_both_bases() {
    let version = scan(&one_folder_with_a_link(), ResultCompleteness::Complete, &[]);
    for basis in [SizeBasis::Logical, SizeBasis::Allocated] {
        let lower = is_lower_bound(&value_of(&version, "x", &basis));
        assert_eq!(is_lower_bound(&value_of(&version, "", &basis)), lower);
        let model = project_at(&version, "", basis.clone(), 16).unwrap();
        assert_eq!(model.shapes.len(), 3, "{basis:?}");
        assert_eq!(model.detail.unknown_items, 0, "{basis:?}");
        let x = row_of(&model, &version, "x");
        assert_eq!(x.lod, Lod::Drawn, "{basis:?}");
        assert_eq!(is_lower_bound(&x.size), lower, "{basis:?}");
        assert!(!is_lower_bound(&row_of(&model, &version, "a").size));
        // The area follows the value, the lower bound or the exact bytes: 20 of 170.
        let share = share_of(&model, &version, "x");
        assert!((share - 20.0 / 170.0).abs() < 1e-3, "{basis:?} {share}");
        // The scope and what is drawn carry the state of the values, and add up as they are.
        assert_eq!(is_lower_bound(&model.totals.scope), lower, "{basis:?}");
        assert_eq!(model.totals.drawn_lower_bound, lower, "{basis:?}");
        assert_eq!(model.totals.drawn_bytes, 170, "{basis:?}");
        assert_eq!(bytes_of(&model.totals.scope), 170, "{basis:?}");
        // A lower-bound parent is no denominator, so no row has a ratio; an exact one is.
        assert_eq!(
            model.rows.iter().all(|row| row.ratio.is_none()),
            lower,
            "{basis:?}"
        );
        assert_eq!(
            model.rows.iter().all(|row| row.ratio.is_some()),
            !lower,
            "{basis:?}"
        );
    }
}

/// The link row has no shape and no value of its own that could give it one: it is an
/// item without a value, which the summary counts, or an exact zero.
fn assert_the_link_has_no_area(
    model: &Model,
    version: &ResultVersion,
    name: &str,
    basis: &SizeBasis,
) {
    let link = row_of(model, version, name);
    let without_value = has_no_value(&value_of(version, name, basis));
    assert_eq!(model.detail.unknown_items, u64::from(without_value));
    let expected = if without_value {
        NoArea::Unknown
    } else {
        NoArea::ExactZero
    };
    assert_eq!(link.omission, Some(expected));
    assert!(
        model
            .shapes
            .iter()
            .all(|shape| model.rows[shape.row_index].item != link.item)
    );
}

#[test]
fn a_link_two_levels_down_is_drawn_at_every_level_by_the_value_above_it() {
    let version = scan(&link_two_levels_down(), ResultCompleteness::Complete, &[]);
    let basis = SizeBasis::Logical;
    let lower = is_lower_bound(&value_of(&version, "deep", &basis));
    let root = project_at(&version, "", basis.clone(), 16).unwrap();
    assert_eq!(root.shapes.len(), 2);
    assert_eq!(is_lower_bound(&row_of(&root, &version, "deep").size), lower);
    assert!(!is_lower_bound(&row_of(&root, &version, "c").size));
    assert_eq!(bytes_of(&root.totals.scope), 170);
    assert_eq!(root.totals.drawn_lower_bound, lower);

    let deep = project_at(&version, "deep", basis.clone(), 16).unwrap();
    assert_eq!(deep.shapes.len(), 2);
    assert_eq!(
        is_lower_bound(&row_of(&deep, &version, "inner").size),
        lower
    );
    assert_eq!(deep.totals.drawn_lower_bound, lower);
    assert_eq!(is_lower_bound(&deep.totals.scope), lower);
    assert_eq!(deep.totals.drawn_bytes, 70);

    // Inside the folder that holds the link the file is drawn, exact, and the link has no
    // shape: it is an item without a value, or an exact zero.
    let inner = project_at(&version, "inner", basis.clone(), 16).unwrap();
    assert_eq!(inner.shapes.len(), 1);
    assert_the_link_has_no_area(&inner, &version, "l", &basis);
    assert_eq!(is_lower_bound(&inner.totals.scope), lower);
    assert!(!inner.totals.drawn_lower_bound);
    assert_eq!(inner.totals.drawn_bytes, 30);
    assert_eq!(inner.rows.iter().all(|row| row.ratio.is_none()), lower);

    // A folder without a link keeps an exact scope, and its rows keep their ratios.
    let clean = project_at(&version, "c", basis, 16).unwrap();
    assert!(!is_lower_bound(&clean.totals.scope));
    assert!(clean.rows.iter().all(|row| row.ratio.is_some()));
}

#[test]
fn an_expanded_folder_above_a_link_draws_its_own_children_by_their_values() {
    let version = scan(&link_two_levels_down(), ResultCompleteness::Complete, &[]);
    let lower = is_lower_bound(&value_of(&version, "deep", &SizeBasis::Logical));
    let mut model = project_at(&version, "", SizeBasis::Logical, 16).unwrap();
    let before = model.totals.clone();
    let deep = find(&version, "deep");
    let shape = model
        .shapes
        .iter()
        .position(|shape| model.rows[shape.row_index].item == deep)
        .unwrap();
    add_child_partition(&mut model, &version, shape, &dispatcher(), 16, &mut || {
        false
    })
    .unwrap();
    // inner and d get shapes inside deep, and the totals stay.
    assert_eq!(model.descendants.len(), 2);
    assert_eq!(
        model
            .descendants
            .iter()
            .any(|shape| is_lower_bound(&shape.size)),
        lower
    );
    assert_eq!(model.totals, before);
    assert!(model.shape_count() <= 16);
}

#[test]
fn a_link_beside_its_siblings_and_a_folder_of_only_links_get_no_area() {
    let version = scan(
        &[
            Entry::Dir("c"),
            Entry::Dir("holder"),
            Entry::File("c/f", 100),
            Entry::Link("holder/jl"),
            Entry::Link("toplink"),
        ],
        ResultCompleteness::Complete,
        &[],
    );
    let basis = SizeBasis::Logical;
    let model = project_at(&version, "", basis.clone(), 16).unwrap();
    // c is drawn; the link and the folder that holds only a link get no shape.
    assert_eq!(model.shapes.len(), 1);
    let none = ["holder", "toplink"]
        .iter()
        .filter(|name| has_no_value(&value_of(&version, name, &basis)))
        .count();
    assert_eq!(model.detail.unknown_items, none as u64);
    for name in ["holder", "toplink"] {
        let row = row_of(&model, &version, name);
        let unknown = has_no_value(&value_of(&version, name, &basis));
        assert_eq!(
            row.omission,
            Some(if unknown {
                NoArea::Unknown
            } else {
                NoArea::ExactZero
            }),
            "{name}"
        );
    }
    assert_eq!(
        is_lower_bound(&model.totals.scope),
        is_lower_bound(&value_of(&version, "", &basis))
    );
    assert_eq!(model.totals.drawn_bytes, 100);
    // A scope with no value of its own cannot be partitioned; an exact zero scope holds
    // only the link, which has no area.
    let holder = project_at(&version, "holder", basis.clone(), 16);
    if has_no_value(&value_of(&version, "holder", &basis)) {
        assert_eq!(
            holder.err(),
            Some(projection::Error::UnavailableAttribution)
        );
    } else {
        let holder = holder.unwrap();
        assert!(holder.shapes.is_empty());
        assert_eq!(holder.totals.drawn_bytes, 0);
    }
}

// What is left over beside a link is nothing: the link has no bytes, so the rest of the
// scope is drawn whole. Whether the link is also counted as an item without a value
// depends on F03's value for it, so the summary may count one unknown item while the
// remainder is zero, or none.
#[test]
fn the_rest_of_a_scope_is_drawn_whole_beside_a_link() {
    let version = scan(
        &[
            Entry::Dir("c"),
            Entry::File("c/f", 100),
            Entry::File("g", 40),
            Entry::Link("toplink"),
        ],
        ResultCompleteness::Complete,
        &[],
    );
    for basis in [SizeBasis::Logical, SizeBasis::Allocated] {
        let model = project_at(&version, "", basis.clone(), 16).unwrap();
        assert_eq!(model.shapes.len(), 2, "{basis:?}");
        let totals = &model.totals;
        assert_eq!(
            bytes_of(&totals.scope) - totals.drawn_bytes - totals.grouped_bytes,
            0,
            "{basis:?}"
        );
        assert_eq!(model.detail.omitted_items, 0, "{basis:?}");
        let unknown = has_no_value(&value_of(&version, "toplink", &basis));
        assert_eq!(model.detail.unknown_items, u64::from(unknown), "{basis:?}");
        let link = row_of(&model, &version, "toplink");
        assert!(
            model
                .shapes
                .iter()
                .all(|shape| model.rows[shape.row_index].item != link.item)
        );
    }
}

#[test]
fn a_shared_file_stays_unattributed_beside_a_sibling_above_a_link() {
    // a and c contain one 10-byte file (two links); b has a file and a link.
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::Dir("b"),
            Entry::Dir("c"),
            Entry::Linked("a/fa", 10, 7, 2),
            Entry::Linked("c/fc", 10, 7, 2),
            Entry::File("b/fb", 20),
            Entry::Link("b/lb"),
        ],
        ResultCompleteness::Complete,
        &[],
    );
    let lower = is_lower_bound(&value_of(&version, "b", &SizeBasis::Allocated));
    let model = project_at(&version, "", SizeBasis::Allocated, 16).unwrap();
    assert_eq!(model.shapes.len(), 1);
    assert_eq!(model.detail.unknown_items, 2);
    for name in ["a", "c"] {
        assert_eq!(
            row_of(&model, &version, name).omission,
            Some(NoArea::Unknown),
            "{name}"
        );
    }
    assert_eq!(is_lower_bound(&row_of(&model, &version, "b").size), lower);
    // The shared file is in the scope once, and in no total.
    assert_eq!(bytes_of(&model.totals.scope), 30);
    assert_eq!(model.totals.drawn_bytes, 20);
}

#[test]
fn a_tail_of_folders_above_links_becomes_a_group_with_the_state_of_its_members() {
    let mut entries = vec![Entry::Dir("big"), Entry::File("big/f", 100_000)];
    let names: Vec<String> = (0..40).map(|i| format!("s{i:02}")).collect();
    for name in &names {
        entries.push(Entry::Dir(name));
    }
    let files: Vec<String> = names.iter().map(|name| format!("{name}/f")).collect();
    let links: Vec<String> = names.iter().map(|name| format!("{name}/l")).collect();
    for (file, link) in files.iter().zip(&links) {
        entries.push(Entry::File(file, 200));
        entries.push(Entry::Link(link));
    }
    let version = scan(&entries, ResultCompleteness::Complete, &[]);
    let lower = is_lower_bound(&value_of(&version, "s00", &SizeBasis::Logical));
    let model = project_at(&version, "", SizeBasis::Logical, 16).unwrap();
    assert_eq!(model.shapes.len(), 1);
    let group = &model.detail.groups[0];
    assert_eq!(group.members.len(), 40);
    assert_eq!(is_lower_bound(&group.total), lower);
    assert_eq!(bytes_of(&group.total), 8_000);
    // The size range is known only when every member is exact.
    assert_eq!(matches!(group.size_range, SizeRange::Known { .. }), !lower);
    assert_eq!(model.totals.grouped_lower_bound, lower);
    assert_eq!(model.totals.grouped_bytes, 8_000);
}
