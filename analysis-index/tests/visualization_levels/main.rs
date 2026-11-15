#![allow(clippy::unwrap_used)]
//! Level-of-detail behavior of the projection: groups, omissions, unattributed
//! children, totals and shape limits.
mod nested;
mod reasons;
mod scale;
mod totals;

use du_analysis_index::{
    FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence, SpaceObservation,
    visualization::{
        geometry::{NoArea, Rect},
        live_models::SizeRange,
        projection::{self, Location, Lod, Model, Options, Source},
        registry,
        typed_layout::{self, Dispatcher, Strategy},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultVersionRead},
};
use std::sync::Arc;

pub fn id(n: u8) -> Id {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).unwrap()
}
pub fn logical(bytes: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}
pub const WINDOW: Rect = Rect {
    left: 0,
    top: 0,
    right: 1_000_000,
    bottom: 1_000_000,
};

/// Builds a result from "dir/name" style paths with logical sizes.
pub fn build(files: &[(String, u64)], completeness: ResultCompleteness) -> Arc<ResultVersion> {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(1)).unwrap();
    let mut dirs: Vec<Vec<Vec<u8>>> = vec![vec![]];
    for (path, _) in files {
        let parts: Vec<Vec<u8>> = path.split('/').map(|p| p.as_bytes().to_vec()).collect();
        for end in 1..parts.len() {
            let dir = parts[..end].to_vec();
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs.sort_by_key(Vec::len);
    for dir in dirs {
        builder.add_directory(dir).unwrap();
    }
    for (path, bytes) in files {
        builder
            .add_file(Observation {
                path: path.split('/').map(|p| p.as_bytes().to_vec()).collect(),
                size: Some(logical(*bytes)),
            })
            .unwrap();
    }
    builder.seal(id(2), completeness).unwrap()
}
pub fn files(entries: &[(&str, u64)]) -> Vec<(String, u64)> {
    entries.iter().map(|(p, b)| ((*p).to_owned(), *b)).collect()
}
pub fn item(version: &ResultVersion, item_no: u32) -> ItemRef {
    ItemRef {
        result_version: version.version_id().clone(),
        item_no,
    }
}
pub fn find(version: &ResultVersion, name: &str) -> ItemRef {
    let found = version
        .items()
        .iter()
        .find(|i| i.name == name.as_bytes())
        .unwrap();
    item(version, found.item_no)
}
pub fn dispatcher() -> Dispatcher {
    let descriptor = typed_layout::treemap_descriptor();
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    builder
        .register(descriptor.clone(), Box::new(typed_layout::Treemap))
        .unwrap();
    Dispatcher::new(builder.build(&[descriptor]).unwrap())
}
pub fn project(
    version: &ResultVersion,
    basis: SizeBasis,
    max_shapes: usize,
) -> Result<Model, projection::Error> {
    project_with(version, basis, max_shapes, true)
}
pub fn project_with(
    version: &ResultVersion,
    basis: SizeBasis,
    max_shapes: usize,
    group_tail: bool,
) -> Result<Model, projection::Error> {
    let parent = item(version, 0);
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
            group_tail,
        },
        &mut || false,
    )
}
pub fn area(geometry: &du_core_contracts::visualization::geometry::Geometry) -> u64 {
    let p = geometry.points();
    u64::from(p[1].x - p[0].x) * u64::from(p[1].y - p[0].y)
}
pub fn bytes_of(size: &SizeValue) -> u64 {
    match &size.amount {
        SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => Some(bytes.get()),
        SizeAmount::Unknown => None,
    }
    .unwrap()
}

fn many(count: usize, bytes: u64) -> Vec<(String, u64)> {
    (0..count).map(|i| (format!("f{i:04}"), bytes)).collect()
}

#[test]
fn equal_siblings_below_the_minimum_area_become_one_group() {
    let version = build(&many(200, 10), ResultCompleteness::Complete);
    let model = project(&version, SizeBasis::Logical, 16).unwrap();
    assert!(model.shapes.is_empty());
    assert_eq!(model.detail.groups.len(), 1);
    let group = &model.detail.groups[0];
    assert_eq!(group.members.len(), 200);
    assert_eq!(bytes_of(&group.total), 2_000);
    assert_eq!(
        group.size_range,
        SizeRange::Known {
            min: logical(10),
            max: logical(10)
        }
    );
    assert_eq!(group.geometry.points()[0].x, 0);
    assert_eq!(group.geometry.points()[1].y, 1_000_000);
    assert_eq!(model.shape_count(), 1);
    assert_eq!(model.totals.drawn_bytes, 0);
    assert_eq!(model.totals.grouped_bytes, 2_000);
    assert_eq!(model.totals.scope, version.space(0).unwrap().logical);
    assert!(model.rows.iter().all(|row| row.lod == Lod::Grouped(0)));
    assert_eq!(model.detail.omitted_items, 0);
    assert_eq!(model.detail.unknown_items, 0);
}

fn mixed(small: usize) -> Vec<(String, u64)> {
    let mut entries = files(&[("big1", 4_000), ("big2", 3_000), ("big3", 2_000)]);
    entries.extend((0..small).map(|i| (format!("s{i:03}"), 10)));
    entries
}

#[test]
fn retained_siblings_precede_one_group_and_the_four_totals_stay_apart() {
    let version = build(&mixed(100), ResultCompleteness::Complete);
    let model = project(&version, SizeBasis::Logical, 16).unwrap();
    assert_eq!(model.shapes.len(), 3);
    assert_eq!(model.detail.groups.len(), 1);
    assert_eq!(model.detail.groups[0].slot, 3);
    assert_eq!(model.totals.drawn_bytes, 9_000);
    assert_eq!(model.totals.grouped_bytes, 1_000);
    assert_eq!(model.totals.filtered_bytes, 0);
    assert_eq!(bytes_of(&model.totals.scope), 10_000);
    assert_eq!(
        model.totals.drawn_bytes + model.totals.grouped_bytes,
        bytes_of(&model.totals.scope)
    );
    assert_eq!(model.shape_count(), 4);
    let lods: Vec<_> = model.rows.iter().map(|row| row.lod).collect();
    assert!(lods[..3].iter().all(|lod| *lod == Lod::Drawn));
    assert!(lods[3..].iter().all(|lod| *lod == Lod::Grouped(0)));
    // The group sits after the three larger siblings and fills the rest of the area.
    let drawn: u64 = model.shapes.iter().map(|s| area(&s.geometry)).sum();
    let grouped = area(&model.detail.groups[0].geometry);
    assert!(drawn + grouped <= 1_000_000_000_000);
    assert!(1_000_000_000_000 - (drawn + grouped) < 3_000_000);
}

#[test]
fn a_tail_below_the_minimum_is_counted_and_leaves_its_share_blank() {
    let mut entries = files(&[("big", 9_000)]);
    entries.extend((0..5).map(|i| (format!("s{i}"), 10)));
    let version = build(&entries, ResultCompleteness::Complete);
    let model = project(&version, SizeBasis::Logical, 16).unwrap();
    assert!(model.detail.groups.is_empty());
    assert_eq!(model.detail.omitted_items, 5);
    assert_eq!(model.shapes.len(), 1);
    // The retained sibling keeps its proportional share of the parent, not the whole.
    let share = area(&model.shapes[0].geometry) as f64 / 1e12;
    assert!((share - 9_000.0 / 9_050.0).abs() < 1e-3, "{share}");
    let omitted = model.rows.iter().filter(|r| r.lod == Lod::Omitted).count();
    assert_eq!(omitted, 5);
    assert_eq!(model.totals.grouped_bytes, 0);
}

#[test]
fn equal_sizes_fall_on_the_same_side_of_the_minimum_area_boundary() {
    // Total 10_000 and 16 shapes: a sibling needs at least 625 bytes.
    for (pair, big, retained, grouped) in [(625, 8_750, 3, 0), (624, 8_752, 1, 2)] {
        let version = build(
            &files(&[("big", big), ("p1", pair), ("p2", pair)]),
            ResultCompleteness::Complete,
        );
        let model = project(&version, SizeBasis::Logical, 16).unwrap();
        assert_eq!(model.shapes.len(), retained, "pair {pair}");
        let members = model.detail.groups.first().map_or(0, |g| g.members.len());
        assert_eq!(members, grouped, "pair {pair}");
    }
}

fn size(basis: SizeBasis, n: u64) -> SizeValue {
    SizeValue {
        evidence: SizeEvidence::OsReported,
        basis,
        amount: SizeAmount::Exact { bytes: n.into() },
    }
}
fn observation(file: u8, links: u64) -> SpaceObservation {
    SpaceObservation {
        allocated: Some(size(SizeBasis::Allocated, 10)),
        identity: Some(FileIdentity {
            volume: vec![1],
            file: vec![file],
        }),
        link_count: Some(links),
        sharing: SharingEvidence::None,
    }
}
// Folders whose only file has the given (identity, links); allocated basis.
fn shared(folders: &[(&str, u8, u64)]) -> Arc<ResultVersion> {
    let sink = IngestSink::default();
    let mut builder = sink.open(id(1)).unwrap();
    builder.add_directory(vec![]).unwrap();
    for (name, _, _) in folders {
        builder
            .add_directory(vec![name.as_bytes().to_vec()])
            .unwrap();
    }
    for (name, file, links) in folders {
        builder
            .add_with_space(
                Observation {
                    path: vec![name.as_bytes().to_vec(), b"file".to_vec()],
                    size: Some(logical(10)),
                },
                observation(*file, *links),
            )
            .unwrap();
    }
    builder.seal(id(2), ResultCompleteness::Complete).unwrap()
}

#[test]
fn siblings_sharing_a_file_are_unattributed_whichever_is_read_first() {
    for folders in [
        [("a", 7, 2), ("b", 8, 1), ("c", 7, 2)],
        [("a", 8, 1), ("b", 7, 2), ("c", 7, 2)],
    ] {
        let version = shared(&folders);
        let model = project(&version, SizeBasis::Allocated, 16).unwrap();
        assert_eq!(model.detail.unknown_items, 2);
        assert_eq!(model.shapes.len(), 1);
        let unattributed: Vec<_> = model
            .rows
            .iter()
            .filter(|row| row.omission == Some(NoArea::Unknown))
            .map(|row| version.items()[row.item.item_no as usize].name.clone())
            .collect();
        let sharing: Vec<_> = folders
            .iter()
            .filter(|f| f.1 == 7)
            .map(|f| f.0.as_bytes().to_vec())
            .collect();
        assert_eq!(unattributed, sharing);
        // The scope keeps its F03 value; the attributed sibling gets its own share only.
        assert_eq!(model.totals.scope, version.space(0).unwrap().allocated);
        assert_eq!(model.totals.drawn_bytes, 10);
        assert_eq!(bytes_of(&model.totals.scope), 20);
        assert_eq!(area(&model.shapes[0].geometry), 500_000_000_000);
    }
}

#[test]
fn a_folder_of_only_unattributed_children_is_a_model_without_shapes() {
    let version = shared(&[("a", 7, 2), ("b", 7, 2)]);
    let model = project(&version, SizeBasis::Allocated, 16).unwrap();
    assert!(model.shapes.is_empty() && model.detail.groups.is_empty());
    assert_eq!(model.detail.unknown_items, 2);
    assert_eq!(model.totals.drawn_bytes, 0);
    assert_eq!(bytes_of(&model.totals.scope), 10);
    assert!(model.rows.iter().all(|row| {
        row.size.amount == size(SizeBasis::Allocated, 10).amount && row.lod == Lod::Drawn
    }));
    assert_eq!(model.omissions.len(), 2);
}

#[test]
fn without_a_group_the_tail_is_only_counted() {
    let version = build(&mixed(100), ResultCompleteness::Complete);
    let model = project_with(&version, SizeBasis::Logical, 16, false).unwrap();
    assert!(model.detail.groups.is_empty());
    assert_eq!(model.shapes.len(), 3);
    assert_eq!(model.totals.grouped_bytes, 0);
    assert_eq!(model.shape_count(), 3);
    assert!(model.rows[3..].iter().all(|row| row.lod == Lod::Omitted));
    assert_eq!(model.detail.omitted_items, 100);
}

#[test]
fn only_a_parent_that_cannot_partition_is_refused() {
    let version = build(&many(5, 10), ResultCompleteness::Complete);
    // No allocated observations exist: the root itself has no partitionable value.
    assert_eq!(
        project(&version, SizeBasis::Allocated, 16).err(),
        Some(projection::Error::UnavailableAttribution)
    );
}

#[test]
fn the_shape_limit_holds_for_every_limit_and_is_never_an_error() {
    let entries: Vec<_> = (0..300)
        .map(|i| (format!("f{i:03}"), (i % 37 + 1) * 11))
        .collect();
    let version = build(&entries, ResultCompleteness::Complete);
    for limit in [1, 2, 3, 7, 16, 100, 400] {
        let first = project(&version, SizeBasis::Logical, limit).unwrap();
        assert!(first.shape_count() <= limit, "limit {limit}");
        let again = project(&version, SizeBasis::Logical, limit).unwrap();
        assert_eq!(first, again);
        assert!(
            first.totals.drawn_bytes + first.totals.grouped_bytes <= bytes_of(&first.totals.scope)
        );
    }
    let one = project(&version, SizeBasis::Logical, 1).unwrap();
    assert!(one.shapes.is_empty() && one.detail.groups.len() == 1);
}

#[test]
fn input_order_does_not_change_the_model() {
    let mut forward = mixed(40);
    let reverse: Vec<_> = forward.iter().cloned().rev().collect();
    let a = build(&forward, ResultCompleteness::Complete);
    let b = build(&reverse, ResultCompleteness::Complete);
    let model_a = project(&a, SizeBasis::Logical, 16).unwrap();
    let model_b = project(&b, SizeBasis::Logical, 16).unwrap();
    assert_eq!(model_a, model_b);
    forward.rotate_left(7);
    let c = build(&forward, ResultCompleteness::Complete);
    assert_eq!(model_a, project(&c, SizeBasis::Logical, 16).unwrap());
}

#[test]
fn group_members_are_located_in_their_group() {
    let version = build(&mixed(100), ResultCompleteness::Complete);
    let model = project(&version, SizeBasis::Logical, 16).unwrap();
    assert_eq!(
        model.locate(&version, &find(&version, "big1")).unwrap(),
        Location::Shape(0)
    );
    assert_eq!(
        model.locate(&version, &find(&version, "s050")).unwrap(),
        Location::Group(model.detail.groups[0].group_ref.clone())
    );
    let omitted = build(
        &[
            files(&[("big", 9_000)]),
            (0..5).map(|i| (format!("s{i}"), 10)).collect(),
        ]
        .concat(),
        ResultCompleteness::Complete,
    );
    let model = project(&omitted, SizeBasis::Logical, 16).unwrap();
    assert_eq!(
        model.locate(&omitted, &find(&omitted, "s3")).unwrap(),
        Location::OutsideView
    );
}

#[test]
fn a_group_of_lower_bound_members_is_a_lower_bound_with_an_unknown_range() {
    let version = build(&many(200, 10), ResultCompleteness::Partial);
    let model = project(&version, SizeBasis::Logical, 16).unwrap();
    let group = &model.detail.groups[0];
    assert!(matches!(group.total.amount, SizeAmount::LowerBound { .. }));
    assert_eq!(group.size_range, SizeRange::Unknown);
    assert!(model.totals.grouped_lower_bound);
    assert!(matches!(
        model.totals.scope.amount,
        SizeAmount::LowerBound { .. }
    ));
}
