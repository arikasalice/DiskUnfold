//! The type filter: pred.file_type in queries and a filtered treemap projection (F06 FR14).
use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    file_type::FileType,
    query::{self, Condition, Predicate},
    visualization::{
        filter::ItemFilter,
        geometry::Rect,
        projection::{self, Options, Source},
        registry,
        typed_layout::{self, Dispatcher, Strategy},
    },
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};
use std::sync::Arc;

type Fixture = (Arc<ResultVersion>, ItemRef, Dispatcher);

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn size(bytes: u64) -> Option<SizeValue> {
    Some(SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    })
}

/// a.mp4 10, b.txt 5, clips/c.mp4 7, clips/d.txt 3, notes/e.txt 4: 29 bytes, 17 of video.
fn error(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

fn fixture() -> Result<Fixture, String> {
    let sink = IngestSink::default();
    // A name is read in its observed root's encoding, so the scope has one observed root.
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut builder = sink.open_forest(id(1)?, vec![root]).map_err(error)?;
    builder.add_directory_at(0, vec![]).map_err(error)?;
    builder
        .add_directory_at(0, vec![b"clips".to_vec()])
        .map_err(error)?;
    builder
        .add_directory_at(0, vec![b"notes".to_vec()])
        .map_err(error)?;
    for (path, bytes) in [
        (vec![b"a.mp4".to_vec()], 10),
        (vec![b"b.txt".to_vec()], 5),
        (vec![b"clips".to_vec(), b"c.mp4".to_vec()], 7),
        (vec![b"clips".to_vec(), b"d.txt".to_vec()], 3),
        (vec![b"notes".to_vec(), b"e.txt".to_vec()], 4),
    ] {
        builder
            .add_file_at(
                0,
                Observation {
                    path,
                    size: size(bytes),
                },
            )
            .map_err(error)?;
    }
    let version = builder
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(error)?;
    let parent = ItemRef {
        result_version: version.version_id().clone(),
        item_no: 0,
    };
    let descriptor = typed_layout::treemap_descriptor();
    let mut registry = registry::Builder::<Box<dyn Strategy>>::default();
    registry
        .register(descriptor.clone(), Box::new(typed_layout::Treemap))
        .map_err(error)?;
    let dispatcher = Dispatcher::new(registry.build(&[descriptor]).map_err(error)?);
    Ok((version, parent, dispatcher))
}

fn type_condition(operand: &[u8]) -> Condition {
    Condition::Predicate(Predicate::Registered {
        id: query::file_type_predicate::ID.to_owned(),
        revision: query::file_type_predicate::REVISION,
        operand: operand.to_vec(),
    })
}

fn by_path() -> query::Sort<'static> {
    query::Sort {
        id: "sort.path",
        revision: 1,
        basis: query::Basis::Logical,
        descending: false,
    }
}

fn names(version: &ResultVersion, items: &[ItemRef]) -> Result<Vec<String>, String> {
    items
        .iter()
        .map(|item| {
            let row = version
                .items()
                .get(item.item_no as usize)
                .ok_or("missing item")?;
            Ok(String::from_utf8_lossy(&row.name).into_owned())
        })
        .collect()
}

#[test]
fn the_type_predicate_matches_the_files_of_its_type_and_no_folder() -> Result<(), String> {
    let (version, _, _) = fixture()?;
    let registry = query::Registry::baseline();
    let condition = type_condition(b"type.video");
    registry
        .prepare_search(&condition, &[by_path()])
        .map_err(|e| format!("{e:?}"))?;
    let found = query::run(&registry, version.clone(), &condition, by_path(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(names(&version, &found.items)?, ["a.mp4", "c.mp4"]);
    Ok(())
}

#[test]
fn only_a_listed_type_key_is_an_operand() {
    let registry = query::Registry::baseline();
    for operand in [b"unclassified".as_slice(), b"type.folder", b"video", b""] {
        assert!(
            registry
                .prepare_search(&type_condition(operand), &[by_path()])
                .is_err(),
            "{}",
            String::from_utf8_lossy(operand)
        );
    }
}

fn project(fixture: &Fixture, filter: ItemFilter) -> Result<projection::Model, String> {
    let (version, parent, dispatcher) = fixture;
    projection::project_filtered(
        Source {
            version,
            parent,
            basis: SizeBasis::Logical,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 1_000_000,
                bottom: 1_000_000,
            },
        },
        dispatcher,
        Options {
            format: "treemap",
            strategy: None,
            max_shapes: 10,
            group_tail: true,
        },
        Some(filter),
        false,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))
}

#[test]
fn a_filtered_treemap_draws_only_the_kept_bytes_and_counts_the_rest_apart() -> Result<(), String> {
    let fixture = fixture()?;
    let model = project(&fixture, ItemFilter::FileType(FileType::Video))?;
    let rows: Vec<_> = model
        .rows
        .iter()
        .map(|row| (row.item.item_no, row.size.amount.clone()))
        .collect();
    let version = &fixture.0;
    assert_eq!(
        names(
            version,
            &model
                .rows
                .iter()
                .map(|row| row.item.clone())
                .collect::<Vec<_>>()
        )?,
        ["a.mp4", "clips"]
    );
    assert_eq!(
        rows.iter()
            .map(|(_, amount)| amount.clone())
            .collect::<Vec<_>>(),
        [
            SizeAmount::Exact { bytes: 10.into() },
            SizeAmount::Exact { bytes: 7.into() },
        ]
    );
    assert_eq!(model.shapes.len(), 2);
    // The scope keeps its original value; the filter removed 5 + 3 + 4 bytes.
    assert_eq!(
        model.totals.scope,
        version.space(0).ok_or("no scope")?.logical
    );
    assert_eq!(model.totals.drawn_bytes, 17);
    assert_eq!(model.totals.filtered_bytes, 12);
    assert_eq!(model.filter, Some(ItemFilter::FileType(FileType::Video)));
    Ok(())
}

#[test]
fn a_type_with_no_file_in_the_scope_is_an_empty_model_not_an_error() -> Result<(), String> {
    let fixture = fixture()?;
    let model = project(&fixture, ItemFilter::FileType(FileType::Archive))?;
    assert!(model.rows.is_empty());
    assert!(model.shapes.is_empty());
    assert_eq!(model.totals.drawn_bytes, 0);
    assert_eq!(model.totals.filtered_bytes, 29);
    Ok(())
}

/// NFR1 measurement for #301, run by hand: the treemap projection of one scope with
/// and without the type filter, at 100 000 and 1 000 000 files. Release build:
/// cargo test --release -p du-analysis-index --test type_filter -- --ignored --nocapture
#[test]
#[ignore = "measurement"]
fn measure_filtered_projection() -> Result<(), String> {
    for folders in [100_u32, 1_000] {
        let root = ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/measure"),
            },
            volume_id: vec![1],
            file_id: vec![2],
        };
        let sink = IngestSink::default();
        let mut builder = sink.open_forest(id(1)?, vec![root]).map_err(error)?;
        builder.add_directory_at(0, vec![]).map_err(error)?;
        let extensions = [".mp4", ".pdf", ".jpg", ".zip", ".rs"];
        for folder in 0..folders {
            let name = format!("f{folder}").into_bytes();
            builder
                .add_directory_at(0, vec![name.clone()])
                .map_err(error)?;
            for file in 0..1_000_u32 {
                let ext = extensions[(file % 5) as usize];
                builder
                    .add_file_at(
                        0,
                        Observation {
                            path: vec![name.clone(), format!("{file}{ext}").into_bytes()],
                            size: size(u64::from(file) + 1),
                        },
                    )
                    .map_err(error)?;
            }
        }
        let version = builder
            .seal(id(2)?, ResultCompleteness::Complete)
            .map_err(error)?;
        let parent = ItemRef {
            result_version: version.version_id().clone(),
            item_no: 0,
        };
        let (_, _, dispatcher) = fixture()?;
        let fixture = (version, parent, dispatcher);
        let mut legend = Vec::new();
        for _ in 0..5 {
            let started = std::time::Instant::now();
            du_analysis_index::visualization::type_legend::scope_totals(
                &fixture.0,
                &fixture.1,
                &SizeBasis::Logical,
                &mut || false,
            )
            .map_err(|e| format!("{e:?}"))?;
            legend.push(started.elapsed());
        }
        legend.sort();
        println!(
            "files {:>9} legend       median {:?} max {:?}",
            folders * 1_000,
            legend[legend.len() / 2],
            legend[legend.len() - 1]
        );
        for filter in [None, Some(ItemFilter::FileType(FileType::Video))] {
            let mut times = Vec::new();
            for _ in 0..5 {
                let started = std::time::Instant::now();
                let (version, parent, dispatcher) = &fixture;
                projection::project_filtered(
                    Source {
                        version,
                        parent,
                        basis: SizeBasis::Logical,
                        bounds: Rect {
                            left: 0,
                            top: 0,
                            right: 1_000_000,
                            bottom: 1_000_000,
                        },
                    },
                    dispatcher,
                    Options {
                        format: "treemap",
                        strategy: None,
                        max_shapes: 2_000,
                        group_tail: true,
                    },
                    filter,
                    false,
                    &mut || false,
                )
                .map_err(|e| format!("{e:?}"))?;
                times.push(started.elapsed());
            }
            times.sort();
            println!(
                "files {:>9} filter {:<5} median {:?} max {:?}",
                folders * 1_000,
                filter.is_some(),
                times[times.len() / 2],
                times[times.len() - 1]
            );
        }
    }
    Ok(())
}

#[test]
fn the_type_legend_counts_the_files_of_the_whole_subtree() -> Result<(), String> {
    use du_analysis_index::visualization::type_legend::{ClassTotal, scope_totals};
    let (version, parent, _) = fixture()?;
    let totals = scope_totals(&version, &parent, &SizeBasis::Logical, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let total = |count, bytes| ClassTotal {
        count,
        bytes,
        lower_bound: false,
    };
    // b.txt, clips/d.txt and notes/e.txt are documents; a.mp4 and clips/c.mp4 videos.
    // Folders are not counted.
    assert_eq!(
        totals.into_iter().collect::<Vec<_>>(),
        [
            ("type.document", total(3, 12)),
            ("type.video", total(2, 17))
        ]
    );
    Ok(())
}
