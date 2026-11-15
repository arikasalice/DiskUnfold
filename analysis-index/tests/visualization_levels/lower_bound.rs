//! Siblings whose F03 value is a lower bound are drawn by that bound, on any result.
//!
//! An untraversed link or an entry that could not be read adds no bytes, so every value
//! above it is a lower bound (F03 FR5.HD1). HLD table 22 draws such a sibling with an
//! area proportional to its lower bound and marks the totals as lower bounds. What has
//! no value of its own, and what the F03 value cannot be matched to, is still counted
//! without an area.
use super::*;
use du_analysis_index::visualization::projection::{Row, add_child_partition};
use du_core_contracts::{
    PathBytes, PathEncoding,
    result::{ResultNativePath, ResultScopeRoot},
};

enum Entry<'a> {
    Dir(&'a str),
    /// A file with its own identity; its allocated size equals its logical size.
    File(&'a str, u64),
    /// A file with a chosen identity and link count: path, bytes, identity, links.
    Linked(&'a str, u64, u8, u64),
    /// A link that was not followed.
    Link(&'a str),
    /// An entry that could not be read: no size and an unknown kind.
    Unread(&'a str),
}

fn scan(
    entries: &[Entry<'_>],
    completeness: ResultCompleteness,
    gaps: &[&str],
) -> Arc<ResultVersion> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let sink = IngestSink::default();
    let mut build = sink.open_observed(id(1), root).unwrap();
    build.add_directory(vec![]).unwrap();
    let path = |text: &str| -> Vec<Vec<u8>> {
        text.split('/')
            .map(|part| part.as_bytes().to_vec())
            .collect()
    };
    let physical = |file: u8, links: u64, bytes: u64| SpaceObservation {
        allocated: Some(size(SizeBasis::Allocated, bytes)),
        identity: Some(FileIdentity {
            volume: vec![1],
            file: vec![file],
        }),
        link_count: Some(links),
        sharing: SharingEvidence::None,
    };
    let mut identity = 100_u8;
    for entry in entries {
        match *entry {
            Entry::Dir(at) => build.add_directory(path(at)).unwrap(),
            Entry::File(at, bytes) => {
                identity += 1;
                build
                    .add_with_space(
                        Observation {
                            path: path(at),
                            size: Some(logical(bytes)),
                        },
                        physical(identity, 1, bytes),
                    )
                    .unwrap();
            }
            Entry::Linked(at, bytes, file, links) => build
                .add_with_space(
                    Observation {
                        path: path(at),
                        size: Some(logical(bytes)),
                    },
                    physical(file, links, bytes),
                )
                .unwrap(),
            Entry::Link(at) => build.add_untraversed_link(path(at)).unwrap(),
            Entry::Unread(at) => build
                .add(Observation {
                    path: path(at),
                    size: None,
                })
                .unwrap(),
        }
    }
    for gap in gaps {
        build
            .record_gap_at(0, path(gap), "scan.path_denied".into(), None)
            .unwrap();
    }
    build.seal(id(2), completeness).unwrap()
}

fn project_at(
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

fn is_lower_bound(size: &SizeValue) -> bool {
    matches!(size.amount, SizeAmount::LowerBound { .. })
}

/// Whether a row that has a shape of its own carries a lower-bound value.
fn drawn_has_a_lower_bound(model: &Model) -> bool {
    model
        .shapes
        .iter()
        .any(|shape| is_lower_bound(&model.rows[shape.row_index].size))
}

fn row_of<'a>(model: &'a Model, version: &ResultVersion, name: &str) -> &'a Row {
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

#[test]
fn a_complete_result_draws_a_folder_with_a_link_by_its_lower_bound_on_both_bases() {
    let version = scan(&one_folder_with_a_link(), ResultCompleteness::Complete, &[]);
    for basis in [SizeBasis::Logical, SizeBasis::Allocated] {
        let space = version.space(find(&version, "x").item_no).unwrap();
        let value = if basis == SizeBasis::Logical {
            &space.logical
        } else {
            &space.allocated
        };
        assert!(is_lower_bound(value), "{basis:?}");
        let model = project_at(&version, "", basis.clone(), 16).unwrap();
        assert_eq!(model.shapes.len(), 3, "{basis:?}");
        assert_eq!(model.detail.unknown_items, 0, "{basis:?}");
        let x = row_of(&model, &version, "x");
        assert!(is_lower_bound(&x.size) && x.lod == Lod::Drawn, "{basis:?}");
        assert!(!is_lower_bound(&row_of(&model, &version, "a").size));
        // The area is proportional to the lower bound: 20 of 170.
        let share = share_of(&model, &version, "x");
        assert!((share - 20.0 / 170.0).abs() < 1e-3, "{basis:?} {share}");
        // The scope and what is drawn are lower bounds, and they add up as they are.
        assert!(is_lower_bound(&model.totals.scope), "{basis:?}");
        assert!(model.totals.drawn_lower_bound, "{basis:?}");
        assert_eq!(model.totals.drawn_bytes, 170, "{basis:?}");
        assert_eq!(bytes_of(&model.totals.scope), 170, "{basis:?}");
        // A lower-bound parent is no denominator, so no row has a ratio.
        assert!(
            model.rows.iter().all(|row| row.ratio.is_none()),
            "{basis:?}"
        );
    }
}

#[test]
fn a_link_two_levels_down_makes_each_ancestor_a_lower_bound_and_every_level_draws() {
    let version = scan(&link_two_levels_down(), ResultCompleteness::Complete, &[]);
    let root = project_at(&version, "", SizeBasis::Logical, 16).unwrap();
    assert_eq!(root.shapes.len(), 2);
    assert!(is_lower_bound(&row_of(&root, &version, "deep").size));
    assert!(!is_lower_bound(&row_of(&root, &version, "c").size));
    assert_eq!(bytes_of(&root.totals.scope), 170);
    assert!(root.totals.drawn_lower_bound);

    let deep = project_at(&version, "deep", SizeBasis::Logical, 16).unwrap();
    assert_eq!(deep.shapes.len(), 2);
    assert!(is_lower_bound(&row_of(&deep, &version, "inner").size));
    assert!(deep.totals.drawn_lower_bound && is_lower_bound(&deep.totals.scope));
    assert_eq!(deep.totals.drawn_bytes, 70);

    // Inside the folder that holds the link, the file is exact and the link is counted.
    let inner = project_at(&version, "inner", SizeBasis::Logical, 16).unwrap();
    assert_eq!(inner.shapes.len(), 1);
    assert_eq!(inner.detail.unknown_items, 1);
    assert!(is_lower_bound(&inner.totals.scope) && !inner.totals.drawn_lower_bound);
    assert_eq!(inner.totals.drawn_bytes, 30);
    assert!(inner.rows.iter().all(|row| row.ratio.is_none()));

    // A folder without a link keeps an exact scope, and its rows keep their ratios.
    let clean = project_at(&version, "c", SizeBasis::Logical, 16).unwrap();
    assert!(!is_lower_bound(&clean.totals.scope));
    assert!(clean.rows.iter().all(|row| row.ratio.is_some()));
}

#[test]
fn an_expanded_lower_bound_folder_draws_its_own_lower_bound_children() {
    let version = scan(&link_two_levels_down(), ResultCompleteness::Complete, &[]);
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
    // inner (a lower bound) and d (exact) get shapes inside deep; the totals stay.
    assert_eq!(model.descendants.len(), 2);
    assert!(
        model
            .descendants
            .iter()
            .any(|shape| is_lower_bound(&shape.size))
    );
    assert_eq!(model.totals, before);
    assert!(model.shape_count() <= 16);
}

#[test]
fn an_entry_without_a_value_is_counted_without_an_area_and_a_folder_of_only_links_is_no_scope() {
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
    let model = project_at(&version, "", SizeBasis::Logical, 16).unwrap();
    // c is drawn; the folder with only a link and the link beside it have no value.
    assert_eq!(model.shapes.len(), 1);
    assert_eq!(model.detail.unknown_items, 2);
    for name in ["holder", "toplink"] {
        let row = row_of(&model, &version, name);
        assert_eq!(row.omission, Some(NoArea::Unknown), "{name}");
        assert_eq!(row.size.amount, SizeAmount::Unknown, "{name}");
    }
    assert!(is_lower_bound(&model.totals.scope));
    assert_eq!(model.totals.drawn_bytes, 100);
    // A scope with no value of its own cannot be partitioned.
    assert_eq!(
        project_at(&version, "holder", SizeBasis::Logical, 16).err(),
        Some(projection::Error::UnavailableAttribution)
    );
}

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
    // The unread entry has no value and no area; the rest is drawn by its own values.
    assert_eq!(model.shapes.len(), 2);
    assert_eq!(model.detail.unknown_items, 1);
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
fn a_shared_file_stays_unattributed_beside_a_lower_bound_sibling() {
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
    let space = version.space(find(&version, "b").item_no).unwrap();
    assert!(is_lower_bound(&space.allocated));
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
    assert!(is_lower_bound(&row_of(&model, &version, "b").size));
    // The shared file is in the scope once, and in no total.
    assert_eq!(bytes_of(&model.totals.scope), 30);
    assert_eq!(model.totals.drawn_bytes, 20);
}

#[test]
fn a_tail_of_lower_bound_folders_becomes_a_lower_bound_group_with_an_unknown_range() {
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
    let model = project_at(&version, "", SizeBasis::Logical, 16).unwrap();
    assert_eq!(model.shapes.len(), 1);
    let group = &model.detail.groups[0];
    assert_eq!(group.members.len(), 40);
    assert!(is_lower_bound(&group.total));
    assert_eq!(bytes_of(&group.total), 8_000);
    assert_eq!(group.size_range, SizeRange::Unknown);
    assert!(model.totals.grouped_lower_bound);
    assert_eq!(model.totals.grouped_bytes, 8_000);
}

#[test]
fn unique_physical_size_keeps_its_exact_only_rule() {
    let version = scan(&one_folder_with_a_link(), ResultCompleteness::Complete, &[]);
    assert_eq!(
        project_at(&version, "", SizeBasis::UniquePhysical, 16).err(),
        Some(projection::Error::UnavailableAttribution)
    );
}
