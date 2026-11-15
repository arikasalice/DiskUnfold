#![allow(clippy::unwrap_used)]
//! The group that merges a treemap parent's small siblings, through the page store.
use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    visualization::{
        geometry::Rect,
        live_models::SizeRange,
        projection::{self, Options, Source},
        registry,
        typed_layout::{self, Dispatcher, Strategy, Treemap},
    },
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    visualization::rows::{VizRow, VizRowsRequest},
};
use du_engine_api::viz_pages::{
    Error, ModelPages,
    production::{self, RowPresentation},
    ranked_group::OtherPresentation,
};
use du_job_runtime::JobIdentity;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

struct Ids(AtomicU64);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        let mut bytes = [0; 16];
        bytes[8..].copy_from_slice(&self.0.fetch_add(1, Ordering::SeqCst).to_be_bytes());
        Id::from_v7_bytes(bytes)
    }
}
fn id(n: u32) -> Id {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}")).unwrap()
}
fn logical(bytes: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}

struct Case {
    sink: IngestSink,
    version: Arc<ResultVersion>,
    model: projection::Model,
}

// Three large files and many small ones: with 16 shapes the small ones merge.
fn case(smalls: &[u64], completeness: ResultCompleteness) -> Case {
    let sink = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut builder = sink.open_observed(id(1), root).unwrap();
    builder.add_directory(vec![]).unwrap();
    let mut entries: Vec<(String, u64)> = vec![
        ("big1".into(), 4_000),
        ("big2".into(), 3_000),
        ("big3".into(), 2_000),
    ];
    entries.extend(
        smalls
            .iter()
            .enumerate()
            .map(|(i, b)| (format!("s{i:04}"), *b)),
    );
    for (name, bytes) in &entries {
        builder
            .add_file(Observation {
                path: vec![name.as_bytes().to_vec()],
                size: Some(logical(*bytes)),
            })
            .unwrap();
    }
    let version = builder.seal(id(2), completeness).unwrap();
    let parent = ItemRef {
        result_version: version.version_id().clone(),
        item_no: 0,
    };
    let descriptor = typed_layout::treemap_descriptor();
    let mut registry = registry::Builder::<Box<dyn Strategy>>::default();
    registry
        .register(descriptor.clone(), Box::new(Treemap))
        .unwrap();
    let dispatcher = Dispatcher::new(registry.build(&[descriptor]).unwrap());
    let model = projection::project(
        Source {
            version: &version,
            parent: &parent,
            basis: SizeBasis::Logical,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 1_000_000,
                bottom: 1_000_000,
            },
        },
        &dispatcher,
        Options {
            format: "treemap",
            strategy: None,
            max_shapes: 16,
            group_tail: true,
        },
        &mut || false,
    )
    .unwrap();
    Case {
        sink,
        version,
        model,
    }
}

fn other(label_key: Option<&str>) -> OtherPresentation {
    OtherPresentation {
        group_ref: "supplier.other".into(),
        display_name: label_key.unwrap_or("Other").into(),
        label_key: label_key.map(str::to_owned),
        legend_class_key: "source.class".into(),
        display_flags: vec!["source.group".into()],
    }
}
fn labels(model: &projection::Model) -> Vec<RowPresentation> {
    model
        .rows
        .iter()
        .map(|row| RowPresentation {
            item: row.item.clone(),
            legend_class_key: "source.class".into(),
            display_flags: vec![],
        })
        .collect()
}
fn pages(case: &Case) -> ModelPages {
    ModelPages::new(
        case.sink.clone(),
        NonZeroUsize::new(3).unwrap(),
        Arc::new(Ids(AtomicU64::new(10))),
    )
}
fn register(
    pages: &ModelPages,
    case: &Case,
    other: Option<&OtherPresentation>,
) -> Result<String, production::Error> {
    production::register_projection(
        pages,
        case.version.clone(),
        &case.model,
        "condition".into(),
        labels(&case.model),
        other,
        &mut || false,
    )
}
fn table(pages: &ModelPages, case: &Case, reference: &str) -> Vec<VizRow> {
    pages
        .rows(&VizRowsRequest {
            model_ref: reference.into(),
            result_revision: case.version.version_id().clone(),
            start: 0,
            count: 1000,
        })
        .unwrap()
        .rows
}
fn bytes(size: &SizeValue) -> u64 {
    match &size.amount {
        SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => bytes.get(),
        SizeAmount::Unknown => 0,
    }
}

fn small_sizes() -> Vec<u64> {
    (0..100).map(|i| 5 + i % 10).collect()
}

#[test]
fn the_group_row_stands_in_for_its_members_and_carries_the_supplied_name() {
    let case = case(&small_sizes(), ResultCompleteness::Complete);
    let group = &case.model.detail.groups[0];
    let presentation = other(Some("ui.visualization.group.other"));
    let pages = pages(&case);
    let reference = register(&pages, &case, Some(&presentation)).unwrap();
    let rows = table(&pages, &case, &reference);
    // Three large files, then the group at its shape's position; no member row.
    assert_eq!(rows.len(), 4);
    let group_row = &rows[group.slot];
    assert_eq!(group_row.item_ref, None);
    assert_eq!(
        group_row.group_ref.as_deref(),
        Some(group.group_ref.as_str())
    );
    // The display name is the supplied name as is, and the label key is its message key.
    assert_eq!(group_row.display_name, presentation.display_name);
    assert_eq!(
        group_row.label_key.as_deref(),
        Some("ui.visualization.group.other")
    );
    assert_eq!(group_row.legend_class_key, "source.class");
    assert_eq!(group_row.display_flags, ["source.group"]);
    assert_eq!(group_row.size, group.total);
    // Item rows carry no label key, and the table never exceeds the scope.
    for row in rows.iter().filter(|row| row.item_ref.is_some()) {
        assert_eq!(row.label_key, None);
    }
    let table_bytes: u64 = rows.iter().map(|row| bytes(&row.size)).sum();
    assert_eq!(table_bytes, bytes(&case.model.totals.scope));
    // Every row, the group row included, keeps the F03 scope as its denominator.
    for row in &rows {
        assert_eq!(
            row.denominator.size.as_ref(),
            Some(&case.model.totals.scope)
        );
    }
}

#[test]
fn a_model_without_a_group_carries_no_label_key_on_any_row() {
    // Only the three large files: no tail, so no group and no presentation is needed.
    let case = case(&[], ResultCompleteness::Complete);
    assert!(case.model.detail.groups.is_empty());
    let pages = pages(&case);
    let reference = register(&pages, &case, None).unwrap();
    let rows = table(&pages, &case, &reference);
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|row| row.item_ref.is_some() && row.label_key.is_none())
    );
}

#[test]
fn group_members_are_read_in_order_by_position_with_the_boundary_counts() {
    let case = case(&small_sizes(), ResultCompleteness::Complete);
    let group = &case.model.detail.groups[0];
    let pages = pages(&case);
    let reference = register(&pages, &case, Some(&other(None))).unwrap();
    let read = |start, count| pages.group_rows(&reference, &group.group_ref, start, count);
    assert_eq!(read(0, 0).err(), Some(Error::InvalidRange));
    assert_eq!(read(0, 1001).err(), Some(Error::InvalidRange));
    assert_eq!(read(0, 1).unwrap().rows.len(), 1);
    let all = read(0, 1000).unwrap();
    assert_eq!(all.total_count, 100);
    assert_eq!(all.rows.len(), 100);
    assert_eq!(all.group_total, group.total);
    assert_eq!(all.result_revision, *case.version.version_id());
    assert_eq!(all.observed_at.as_ref(), case.version.observed_at());
    assert_eq!(all.completeness, ResultCompleteness::Complete);
    // The page order is the projection's member order, and members never carry a label key.
    let expected: Vec<_> = group.members.iter().map(|m| m.item.clone()).collect();
    let paged: Vec<_> = all
        .rows
        .iter()
        .filter_map(|row| row.item_ref.clone())
        .collect();
    assert_eq!(paged, expected);
    assert!(all.rows.iter().all(|row| row.label_key.is_none()));
    let sizes: Vec<u64> = all.rows.iter().map(|row| bytes(&row.size)).collect();
    assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]));
    // The ends of the range: an empty page at the end, an error past it.
    assert_eq!(read(100, 5).unwrap().rows.len(), 0);
    assert_eq!(read(99, 1000).unwrap().rows.len(), 1);
    assert_eq!(read(101, 5).err(), Some(Error::InvalidRange));
    assert_eq!(
        pages.group_rows(&reference, "no-such-group", 0, 5).err(),
        Some(Error::InvalidGroup)
    );
    assert_eq!(
        pages
            .group_rows("no-such-model", &group.group_ref, 0, 5)
            .err(),
        Some(Error::ModelExpired)
    );
    assert_eq!(
        all.size_range,
        SizeRange::Known {
            min: logical(5),
            max: logical(14)
        }
    );
}

#[test]
fn a_group_of_lower_bound_members_reads_an_unknown_range() {
    let case = case(&small_sizes(), ResultCompleteness::Partial);
    let group = &case.model.detail.groups[0];
    let pages = pages(&case);
    let reference = register(&pages, &case, Some(&other(None))).unwrap();
    let page = pages
        .group_rows(&reference, &group.group_ref, 0, 5)
        .unwrap();
    assert_eq!(page.size_range, SizeRange::Unknown);
    assert!(matches!(
        page.group_total.amount,
        SizeAmount::LowerBound { .. }
    ));
    assert_eq!(page.completeness, ResultCompleteness::Partial);
}

#[test]
fn a_label_key_must_be_the_display_name_of_the_group_row() {
    let case = case(&small_sizes(), ResultCompleteness::Complete);
    let pages = pages(&case);
    // The display name is required, so it carries the key for a consumer that does not
    // read the key; a different name would show one string and look up another.
    let mut renamed = other(Some("ui.visualization.group.other"));
    renamed.display_name = "Other".into();
    assert_eq!(
        register(&pages, &case, Some(&renamed)).err(),
        Some(production::Error::MissingGroupPresentation)
    );
    // A name the supplier chose has no key, and an equal pair is accepted.
    assert!(register(&pages, &case, Some(&other(None))).is_ok());
    assert!(
        register(
            &pages,
            &case,
            Some(&other(Some("ui.visualization.group.other")))
        )
        .is_ok()
    );
}

#[test]
fn a_group_without_a_usable_presentation_or_with_forged_values_is_refused() {
    let case = case(&small_sizes(), ResultCompleteness::Complete);
    let pages = pages(&case);
    assert_eq!(
        register(&pages, &case, None).err(),
        Some(production::Error::MissingGroupPresentation)
    );
    let mut unnamed = other(None);
    unnamed.display_name.clear();
    let mut empty_label = other(None);
    empty_label.label_key = Some(String::new());
    for bad in [unnamed, empty_label] {
        assert_eq!(
            register(&pages, &case, Some(&bad)).err(),
            Some(production::Error::MissingGroupPresentation)
        );
    }
    let mut forged = case.model.clone();
    forged.detail.groups[0].total = logical(1);
    let result = production::register_projection(
        &pages,
        case.version.clone(),
        &forged,
        "forged".into(),
        labels(&forged),
        Some(&other(None)),
        &mut || false,
    );
    assert_eq!(result.err(), Some(production::Error::InvalidGroup));
    let mut stray = case.model.clone();
    stray.totals.grouped_bytes += 1;
    stray.detail.groups.clear();
    let result = production::register_projection(
        &pages,
        case.version.clone(),
        &stray,
        "stray".into(),
        labels(&stray),
        Some(&other(None)),
        &mut || false,
    );
    assert_eq!(result.err(), Some(production::Error::InvalidGroup));
}
