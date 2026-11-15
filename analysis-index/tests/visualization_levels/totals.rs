//! What the totals of a treemap level mean and what the gap between them holds.
//!
//! The scope is the F03 aggregate. The drawn total adds the root siblings that have a
//! shape of their own and the grouped total adds the members of the root group; the two
//! sets are disjoint, and nested shapes lie inside their folder and are never added
//! again. What neither total holds is the remainder: siblings left out below the
//! minimum area or for coordinate precision, plus the share of the scope that no single
//! sibling can carry because two of them contain the same file.
use super::*;

/// Scope minus the drawn, grouped and filtered bytes.
fn remainder(model: &Model) -> u64 {
    let totals = &model.totals;
    bytes_of(&totals.scope) - totals.drawn_bytes - totals.grouped_bytes - totals.filtered_bytes
}

/// Bytes of the attributed siblings that have neither a shape nor a group.
fn omitted_bytes(model: &Model) -> u64 {
    model
        .rows
        .iter()
        .filter(|row| row.lod == Lod::Omitted || row.omission == Some(NoArea::CoordinatePrecision))
        .map(|row| bytes_of(&row.size))
        .sum()
}

fn unattributed_rows(model: &Model) -> u64 {
    model
        .rows
        .iter()
        .filter(|row| row.omission == Some(NoArea::Unknown))
        .map(|row| bytes_of(&row.size))
        .sum()
}

fn shaped_bytes(model: &Model) -> u64 {
    model
        .shapes
        .iter()
        .map(|shape| bytes_of(&model.rows[shape.row_index].size))
        .sum()
}

fn grouped_member_bytes(model: &Model) -> u64 {
    model
        .rows
        .iter()
        .filter(|row| matches!(row.lod, Lod::Grouped(_)))
        .map(|row| bytes_of(&row.size))
        .sum()
}

fn varied(count: usize) -> Vec<(String, u64)> {
    (0..count)
        .map(|i| (format!("f{i:03}"), (i as u64 % 37 + 1) * 11))
        .collect()
}

#[test]
fn the_drawn_and_grouped_totals_add_disjoint_sets_of_root_siblings() {
    let fixtures = [
        build(&many(5, 10), ResultCompleteness::Complete),
        build(&many(200, 10), ResultCompleteness::Complete),
        build(&mixed(100), ResultCompleteness::Complete),
        build(&varied(300), ResultCompleteness::Complete),
    ];
    for version in &fixtures {
        for limit in [1, 3, 16, 400] {
            let model = project(version, SizeBasis::Logical, limit).unwrap();
            let totals = &model.totals;
            // A sibling is drawn, grouped or neither; no row is in two places.
            assert!(
                model
                    .shapes
                    .iter()
                    .all(|shape| model.rows[shape.row_index].lod == Lod::Drawn),
                "limit {limit}"
            );
            assert_eq!(totals.drawn_bytes, shaped_bytes(&model), "limit {limit}");
            assert_eq!(
                totals.grouped_bytes,
                grouped_member_bytes(&model),
                "limit {limit}"
            );
            let group_total = model
                .detail
                .groups
                .first()
                .map_or(0, |g| bytes_of(&g.total));
            assert_eq!(totals.grouped_bytes, group_total, "limit {limit}");
            assert!(
                totals.drawn_bytes + totals.grouped_bytes <= bytes_of(&totals.scope),
                "limit {limit}"
            );
            assert_eq!(totals.filtered_bytes, 0);
        }
    }
}

#[test]
fn without_omissions_or_unknowns_the_drawn_and_grouped_bytes_are_the_whole_scope() {
    for (entries, limit) in [
        (many(5, 10), 16),
        (many(200, 10), 16),
        (mixed(100), 16),
        (mixed(100), 400),
    ] {
        let version = build(&entries, ResultCompleteness::Complete);
        let model = project(&version, SizeBasis::Logical, limit).unwrap();
        assert_eq!(model.detail.omitted_items, 0, "limit {limit}");
        assert_eq!(model.detail.unknown_items, 0, "limit {limit}");
        assert_eq!(remainder(&model), 0, "limit {limit}");
    }
}

#[test]
fn the_remainder_is_the_bytes_of_the_omitted_siblings() {
    // Fifty bytes of small siblings are below the minimum area and have no group.
    let mut entries = files(&[("big", 9_000)]);
    entries.extend((0..5).map(|i| (format!("s{i}"), 10)));
    let version = build(&entries, ResultCompleteness::Complete);
    let model = project(&version, SizeBasis::Logical, 16).unwrap();
    assert_eq!(model.detail.omitted_items, 5);
    assert_eq!(remainder(&model), 50);
    assert_eq!(remainder(&model), omitted_bytes(&model));

    // With the group switched off, the whole tail of a larger model is the remainder.
    let version = build(&mixed(100), ResultCompleteness::Complete);
    let model = project_with(&version, SizeBasis::Logical, 16, false).unwrap();
    assert_eq!(model.totals.grouped_bytes, 0);
    assert_eq!(model.detail.omitted_items, 100);
    assert_eq!(remainder(&model), 1_000);
    assert_eq!(remainder(&model), omitted_bytes(&model));

    // A group that itself falls below the minimum area is omitted the same way.
    let version = build(&varied(300), ResultCompleteness::Complete);
    for limit in [1, 2, 3, 7, 16, 100] {
        let model = project(&version, SizeBasis::Logical, limit).unwrap();
        assert_eq!(remainder(&model), omitted_bytes(&model), "limit {limit}");
        assert_eq!(model.detail.unknown_items, 0, "limit {limit}");
    }
}

fn physical(file: u8, links: u64, bytes: u64) -> SpaceObservation {
    SpaceObservation {
        allocated: Some(size(SizeBasis::Allocated, bytes)),
        identity: Some(FileIdentity {
            volume: vec![1],
            file: vec![file],
        }),
        link_count: Some(links),
        sharing: SharingEvidence::None,
    }
}

// One folder per entry, each holding one file: (folder, file identity, links, bytes).
fn allocated_folders(folders: &[(&str, u8, u64, u64)]) -> Arc<ResultVersion> {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(1)).unwrap();
    builder.add_directory(vec![]).unwrap();
    for (name, ..) in folders {
        builder
            .add_directory(vec![name.as_bytes().to_vec()])
            .unwrap();
    }
    for (name, file, links, bytes) in folders {
        builder
            .add_with_space(
                Observation {
                    path: vec![name.as_bytes().to_vec(), b"file".to_vec()],
                    size: Some(logical(*bytes)),
                },
                physical(*file, *links, *bytes),
            )
            .unwrap();
    }
    builder.seal(id(2), ResultCompleteness::Complete).unwrap()
}

#[test]
fn a_file_shared_by_two_siblings_stays_in_the_remainder_once() {
    // a and c contain the same 10-byte file, which the scope counts once.
    let version = allocated_folders(&[("a", 7, 2, 10), ("b", 8, 1, 10), ("c", 7, 2, 10)]);
    let model = project(&version, SizeBasis::Allocated, 16).unwrap();
    assert_eq!(model.detail.unknown_items, 2);
    assert_eq!(bytes_of(&model.totals.scope), 20);
    assert_eq!(model.totals.drawn_bytes, 10);
    assert_eq!(model.totals.grouped_bytes, 0);
    assert_eq!(remainder(&model), 10);
    // The rows of the two siblings add up to more than the share they carry: their
    // values are shown but never added to a total.
    assert_eq!(unattributed_rows(&model), 20);
    assert_eq!(model.detail.omitted_items, 0);
}

#[test]
fn omitted_siblings_and_an_unattributed_share_add_up_in_the_remainder() {
    // 600 bytes of shared file, one large sibling and four siblings below the minimum.
    let version = allocated_folders(&[
        ("a", 7, 2, 600),
        ("b", 7, 2, 600),
        ("big", 8, 1, 8_000),
        ("t1", 11, 1, 10),
        ("t2", 12, 1, 10),
        ("t3", 13, 1, 10),
        ("t4", 14, 1, 10),
    ]);
    let model = project_with(&version, SizeBasis::Allocated, 16, false).unwrap();
    assert_eq!(model.detail.unknown_items, 2);
    assert_eq!(model.detail.omitted_items, 4);
    assert_eq!(bytes_of(&model.totals.scope), 8_640);
    assert_eq!(model.totals.drawn_bytes, 8_000);
    assert_eq!(remainder(&model), 640);
    // The omitted siblings carry 40 of them; the 600 are the shared file.
    assert_eq!(omitted_bytes(&model), 40);
    assert_eq!(remainder(&model) - omitted_bytes(&model), 600);
}

fn with_link(completeness: ResultCompleteness) -> Arc<ResultVersion> {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(1)).unwrap();
    for dir in [
        vec![],
        vec![b"a".to_vec()],
        vec![b"b".to_vec()],
        vec![b"x".to_vec()],
    ] {
        builder.add_directory(dir).unwrap();
    }
    for (folder, bytes) in [("a", 100), ("b", 50), ("x", 20)] {
        builder
            .add_file(Observation {
                path: vec![folder.as_bytes().to_vec(), b"file".to_vec()],
                size: Some(logical(bytes)),
            })
            .unwrap();
    }
    builder
        .add_untraversed_link(vec![b"x".to_vec(), b"link".to_vec()])
        .unwrap();
    builder.seal(id(2), completeness).unwrap()
}

fn project_in(version: &ResultVersion, parent: &ItemRef) -> Result<Model, projection::Error> {
    projection::project(
        Source {
            version,
            parent,
            basis: SizeBasis::Logical,
            bounds: WINDOW,
        },
        &dispatcher(),
        Options {
            format: "treemap",
            strategy: None,
            max_shapes: 16,
            group_tail: true,
        },
        &mut || false,
    )
}

fn is_lower_bound(size: &SizeValue) -> bool {
    matches!(size.amount, SizeAmount::LowerBound { .. })
}

// F03 values the folders above an untraversed link as lower bounds today, and keeps them
// exact once FR5.HD1 counts a link without lowering the value; a partial result is a lower
// bound everywhere either way. The level draws a sibling by its value, so the totals add
// up as they are, and the state of the scope and of the drawn total is the state of the
// values: these tests read the value from F03 and hold for both.
#[test]
fn a_scope_above_a_link_is_drawn_by_its_values_in_complete_and_partial_results() {
    for completeness in [ResultCompleteness::Complete, ResultCompleteness::Partial] {
        let version = with_link(completeness);
        let lower = is_lower_bound(&version.space(0).unwrap().logical);
        assert!(lower || completeness == ResultCompleteness::Complete);
        let model = project(&version, SizeBasis::Logical, 16).unwrap();
        // x holds a link; the link is inside x, so nothing at this level has no value.
        assert_eq!(model.shapes.len(), 3, "{completeness:?}");
        assert_eq!(model.detail.unknown_items, 0, "{completeness:?}");
        assert_eq!(model.detail.omitted_items, 0, "{completeness:?}");
        assert_eq!(
            is_lower_bound(&model.totals.scope),
            lower,
            "{completeness:?}"
        );
        assert_eq!(model.totals.drawn_lower_bound, lower, "{completeness:?}");
        assert_eq!(model.totals.drawn_bytes, 170, "{completeness:?}");
        assert_eq!(bytes_of(&model.totals.scope), 170, "{completeness:?}");
        assert_eq!(remainder(&model), 0, "{completeness:?}");
        let x = find(&version, "x");
        let row = model.rows.iter().find(|row| row.item == x).unwrap();
        assert_eq!(row.lod, Lod::Drawn);
        assert_eq!(bytes_of(&row.size), 20);
    }
}

// The state of a total follows its own members: the scope of x has the state of its value
// because of the link, and what is drawn in it is an exact file.
#[test]
fn a_scope_above_a_link_keeps_the_state_of_its_value_and_an_exact_drawn_total() {
    let version = with_link(ResultCompleteness::Complete);
    let space = |name: &str| {
        let folder = find(&version, name);
        version.space(folder.item_no).unwrap().logical.clone()
    };
    let lower = is_lower_bound(&space("x"));
    assert_eq!(is_lower_bound(&version.space(0).unwrap().logical), lower);
    assert_eq!(space("a").amount, logical(100).amount);
    let model = project_in(&version, &find(&version, "x")).unwrap();
    assert_eq!(model.shapes.len(), 1);
    // The link has no shape: it is an item without a value, or an exact zero.
    let link_has_no_value = space("link").amount == SizeAmount::Unknown;
    assert_eq!(model.detail.unknown_items, u64::from(link_has_no_value));
    assert_eq!(is_lower_bound(&model.totals.scope), lower);
    assert!(!model.totals.drawn_lower_bound);
    assert_eq!(model.totals.drawn_bytes, 20);
    assert_eq!(bytes_of(&model.totals.scope), 20);
    // A folder without a link keeps an exact scope and an exact drawn total.
    let model = project_in(&version, &find(&version, "a")).unwrap();
    assert!(!is_lower_bound(&model.totals.scope) && !model.totals.drawn_lower_bound);
}
