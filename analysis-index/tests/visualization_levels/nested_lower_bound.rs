//! The small children of an expanded folder, when the folder or its members are lower
//! bounds, and when a range the scan could not read covers the folder.
//!
//! An expanded folder is partitioned by the same per-child admission as the scope, so a
//! lower-bound folder is split by its lower bound, its tail merges into a group that has
//! the state of its members, and a folder that cannot be partitioned stays hidden behind
//! its child count. The value state is read from F03, so each test holds whichever way
//! F03 values what a link touches.
use super::lower_bound::{Entry, is_lower_bound, row_of, scan, value_of};
use super::*;
use du_analysis_index::visualization::projection::{add_child_partition, finish_child_partitions};

const LIMIT: usize = 16;

fn expand(model: &mut Model, version: &ResultVersion, name: &str) -> Result<(), projection::Error> {
    let wanted = find(version, name);
    let index = model
        .shapes
        .iter()
        .position(|shape| model.rows[shape.row_index].item == wanted)
        .unwrap();
    add_child_partition(model, version, index, &dispatcher(), LIMIT, &mut || false)
}

fn names(prefix: &str) -> Vec<String> {
    (0..40).map(|i| format!("{prefix}{i:02}")).collect()
}

// Folder p holds a large file, an entry that could not be read, and forty small folders
// that each hold a file and an entry that could not be read. Every one of them is a
// lower bound. Each of the forty is below the minimum area, and together they are above it.
#[test]
fn the_tail_of_a_lower_bound_folder_becomes_a_group_with_the_state_of_its_members() {
    let dirs: Vec<String> = names("s").iter().map(|n| format!("p/{n}")).collect();
    let files: Vec<String> = dirs.iter().map(|d| format!("{d}/f")).collect();
    let gone: Vec<String> = dirs.iter().map(|d| format!("{d}/u")).collect();
    let mut entries = vec![
        Entry::Dir("p"),
        Entry::File("p/big", 100_000),
        Entry::Unread("p/gone"),
        Entry::File("q", 50_000),
    ];
    for ((dir, file), unread) in dirs.iter().zip(&files).zip(&gone) {
        entries.push(Entry::Dir(dir));
        entries.push(Entry::File(file, 500));
        entries.push(Entry::Unread(unread));
    }
    let version = scan(&entries, ResultCompleteness::Complete, &[]);
    assert!(is_lower_bound(&value_of(
        &version,
        "p",
        &SizeBasis::Logical
    )));
    let mut model =
        super::lower_bound::project_at(&version, "", SizeBasis::Logical, LIMIT).unwrap();
    let before = model.clone();
    expand(&mut model, &version, "p").unwrap();
    finish_child_partitions(&model, &mut || false).unwrap();

    // The forty members merge into one group below p. The total is a lower bound because
    // its members are, so its size range is not known.
    assert_eq!(model.detail.groups.len(), 1);
    let group = &model.detail.groups[0];
    assert_eq!(group.parent, find(&version, "p"));
    assert_eq!(group.members.len(), 40);
    assert!(
        group
            .members
            .iter()
            .all(|member| is_lower_bound(&member.size))
    );
    assert!(is_lower_bound(&group.total));
    assert_eq!(bytes_of(&group.total), 20_000);
    assert_eq!(group.size_range, SizeRange::Unknown);
    // The large file is drawn by its exact value, and the entry that could not be read is
    // counted without an area.
    assert_eq!(model.descendants.len(), 1);
    assert!(!is_lower_bound(&model.descendants[0].size));
    assert_eq!(model.detail.unknown_items, 1);
    // A group inside a drawn folder lies inside that folder's bytes: no total moves, and
    // the members are no longer hidden children of p.
    assert_eq!(model.totals, before.totals);
    assert_eq!(model.shapes, before.shapes);
    assert_eq!(row_of(&model, &version, "p").omitted_children, 0);
    assert!(model.shape_count() <= LIMIT);
}

// The same folder with a tail of plain files: the folder is a lower bound, its members
// are exact, and the group takes the state of its members, not of its parent.
#[test]
fn an_exact_tail_inside_a_lower_bound_folder_keeps_an_exact_group_and_a_known_range() {
    let files: Vec<String> = names("f").iter().map(|n| format!("p/{n}")).collect();
    let mut entries = vec![
        Entry::Dir("p"),
        Entry::File("p/big", 100_000),
        Entry::Unread("p/gone"),
        Entry::File("q", 50_000),
    ];
    entries.extend(files.iter().map(|file| Entry::File(file, 500)));
    let version = scan(&entries, ResultCompleteness::Complete, &[]);
    assert!(is_lower_bound(&value_of(
        &version,
        "p",
        &SizeBasis::Logical
    )));
    let mut model =
        super::lower_bound::project_at(&version, "", SizeBasis::Logical, LIMIT).unwrap();
    expand(&mut model, &version, "p").unwrap();

    assert!(is_lower_bound(&row_of(&model, &version, "p").size));
    let group = &model.detail.groups[0];
    assert_eq!(group.members.len(), 40);
    assert!(!is_lower_bound(&group.total));
    assert_eq!(bytes_of(&group.total), 20_000);
    assert_eq!(
        group.size_range,
        SizeRange::Known {
            min: logical(500),
            max: logical(500)
        }
    );
    assert_eq!(model.detail.unknown_items, 1);
}

// A folder inside a range that could not be read is not partitioned at the scope and is
// not partitioned when it is expanded: no shape is invented for its children, nothing
// else in the model changes, and the children stay behind their count.
#[test]
fn an_expanded_folder_inside_an_unread_range_stays_hidden_behind_its_count() {
    let version = scan(
        &[
            Entry::Dir("a"),
            Entry::File("a/fa", 100),
            Entry::File("a/fb", 60),
            Entry::File("top", 50),
        ],
        ResultCompleteness::Missing,
        &["a"],
    );
    let mut model =
        super::lower_bound::project_at(&version, "", SizeBasis::Logical, LIMIT).unwrap();
    assert_eq!(model.shapes.len(), 2);
    let before = model.clone();
    expand(&mut model, &version, "a").unwrap();
    assert_eq!(model, before);
    assert_eq!(row_of(&model, &version, "a").omitted_children, 2);
}
