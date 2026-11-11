use du_analysis_index::{
    FileIdentity, IngestSink, Observation, ResultVersion, SharingEvidence, SpaceObservation,
    visualization::{
        geometry::{Rect, Scale},
        histogram::Bin,
    },
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    visualization::rows::VizRowsRequest,
};
use du_engine_api::viz_pages::{
    Error, ModelPages,
    histogram::{self, BinPresentation},
    production::RowPresentation,
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
        Id::from_v7_bytes([self.0.fetch_add(1, Ordering::SeqCst) as u8; 16])
    }
}

#[test]
fn group_total_uses_f03_deduplication_instead_of_summing_file_rows() -> Result<(), String> {
    let sink = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/test"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_forest(id(4), vec![root])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    for name in [b"a".as_slice(), b"b".as_slice()] {
        build
            .add_with_space_at(
                0,
                Observation {
                    path: vec![name.to_vec()],
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact {
                            bytes: 5_u64.into(),
                        },
                    }),
                },
                SpaceObservation {
                    allocated: Some(SizeValue {
                        basis: SizeBasis::Allocated,
                        evidence: SizeEvidence::OsReported,
                        amount: SizeAmount::Exact {
                            bytes: 10_u64.into(),
                        },
                    }),
                    identity: Some(FileIdentity {
                        volume: vec![1],
                        file: vec![2],
                    }),
                    link_count: Some(2),
                    sharing: SharingEvidence::None,
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    let version = build
        .seal(id(5), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let scope = ItemRef {
        result_version: id(5),
        item_no: 0,
    };
    let bins = [Bin {
        lower: 0,
        upper: None,
    }];
    let candidate = histogram::prepare(
        histogram::Request {
            version: version.clone(),
            scope: &scope,
            size_basis: SizeBasis::Allocated,
            bins: &bins,
            bounds: Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            scale: Scale::Automatic,
            fingerprint: "hardlinks".into(),
            unknown_reason: "fixture.unknown".into(),
            bin_presentation: vec![BinPresentation {
                group_ref: "g".into(),
                display_name: "bin".into(),
                legend_class_key: "c".into(),
                display_flags: vec![],
            }],
            item_presentation: (1..=2)
                .map(|n| RowPresentation {
                    item: ItemRef {
                        result_version: id(5),
                        item_no: n,
                    },
                    legend_class_key: "c".into(),
                    display_flags: vec![],
                })
                .collect(),
        },
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let group = candidate.pages.groups.get("g").ok_or("group")?;
    assert_eq!(group.rows.len(), 2);
    assert!(group.rows.iter().all(|row| row.size.amount
        == SizeAmount::Exact {
            bytes: 10_u64.into()
        }));
    assert_eq!(
        group.total.amount,
        SizeAmount::Exact {
            bytes: 10_u64.into()
        }
    );
    assert_eq!(
        candidate.pages.rows[0]
            .quantity
            .as_ref()
            .ok_or("count")?
            .unit(),
        "items"
    );
    reject_nonadditive_byte_geometry(version, &scope)?;
    Ok(())
}
fn reject_nonadditive_byte_geometry(
    version: Arc<ResultVersion>,
    scope: &ItemRef,
) -> Result<(), String> {
    use du_analysis_index::visualization::{
        bundle,
        histogram::Basis,
        histogram_dispatch, registry,
        typed_layout::{Dispatcher, Strategy},
    };
    use du_core_contracts::size_units::{SI_UNITS, UNIT_TABLE_REVISION};
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_histogram_candidate(&mut builder).map_err(|e| format!("{e:?}"))?;
    let dispatcher = Dispatcher::new(builder.build(&[]).map_err(|e| format!("{e:?}"))?);
    let bounds = Rect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
    };
    let selected = histogram::PinnedRendered::compute(
        version.clone(),
        histogram_dispatch::Request {
            version: &version,
            scope,
            size_basis: SizeBasis::Allocated,
            unit_table: &SI_UNITS,
            unit_table_revision: UNIT_TABLE_REVISION,
            bounds,
            basis: Basis::Bytes,
            scale: Scale::Automatic,
            unknown_reason: "fixture.unknown",
        },
        &dispatcher,
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let bins = selected.rendered().prepared().unit_bins().bins.clone();
    let request = histogram::Request {
        version,
        scope,
        size_basis: SizeBasis::Allocated,
        bins: &bins,
        bounds,
        scale: Scale::Automatic,
        fingerprint: "hardlinks-bytes".into(),
        unknown_reason: "fixture.unknown".into(),
        bin_presentation: (0..bins.len())
            .map(|i| BinPresentation {
                group_ref: format!("g{i}"),
                display_name: format!("bin {i}"),
                legend_class_key: "c".into(),
                display_flags: vec![],
            })
            .collect(),
        item_presentation: (1..=2)
            .map(|n| RowPresentation {
                item: ItemRef {
                    result_version: id(5),
                    item_no: n,
                },
                legend_class_key: "c".into(),
                display_flags: vec![],
            })
            .collect(),
    };
    assert!(matches!(
        histogram::prepare_selected(request, selected, &mut || false),
        Err(histogram::Error::Source)
    ));
    Ok(())
}
fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}
fn publish(sink: &IngestSink, version: u8) -> Result<Arc<ResultVersion>, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/test"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_forest(id(1), vec![root])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    for (name, bytes) in [(b"a".as_slice(), 5), (b"b".as_slice(), 15)] {
        build
            .add_file_at(
                0,
                Observation {
                    path: vec![name.to_vec()],
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact {
                            bytes: bytes.into(),
                        },
                    }),
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(version), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))
}
#[test]
fn candidate_pages_share_members_and_obey_source_and_model_lifetimes() -> Result<(), String> {
    let sink = IngestSink::default();
    let version = publish(&sink, 2)?;
    let scope = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let bins = [
        Bin {
            lower: 0,
            upper: Some(10),
        },
        Bin {
            lower: 10,
            upper: None,
        },
    ];
    let request = || histogram::Request {
        version: version.clone(),
        scope: &scope,
        size_basis: SizeBasis::Logical,
        bins: &bins,
        bounds: Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        scale: Scale::Automatic,
        fingerprint: "fixture".into(),
        unknown_reason: "fixture.unknown".into(),
        bin_presentation: (0..2)
            .map(|n| BinPresentation {
                group_ref: format!("group-{n}"),
                display_name: format!("bin {n}"),
                legend_class_key: "fixture.class".into(),
                display_flags: vec![],
            })
            .collect(),
        item_presentation: (1..=2)
            .map(|n| RowPresentation {
                item: ItemRef {
                    result_version: id(2),
                    item_no: n,
                },
                legend_class_key: "fixture.class".into(),
                display_flags: vec![],
            })
            .collect(),
    };
    assert!(matches!(
        histogram::prepare(request(), &mut || true),
        Err(histogram::Error::Cancelled)
    ));
    let candidate = histogram::prepare(request(), &mut || false).map_err(|e| format!("{e:?}"))?;
    let mut changed = request();
    changed.bin_presentation[0].display_name = "different label".into();
    let changed = histogram::prepare(changed, &mut || false).map_err(|e| format!("{e:?}"))?;
    assert_ne!(
        candidate.pages.condition_fingerprint,
        changed.pages.condition_fingerprint
    );
    let mut unavailable = request();
    unavailable.size_basis = SizeBasis::UniquePhysical;
    unavailable.item_presentation.clear();
    let unknown = histogram::prepare(unavailable, &mut || false).map_err(|e| format!("{e:?}"))?;
    assert!(
        unknown
            .model
            .histogram
            .bins
            .iter()
            .all(|bin| bin.members.is_empty())
    );
    assert_eq!(unknown.model.histogram.uncertain_items.len(), 2);
    assert!(
        unknown
            .pages
            .rows
            .iter()
            .all(|row| row.size.amount == SizeAmount::Unknown)
    );
    assert_eq!(candidate.model.histogram.bins[0].members, vec![1]);
    let pages = ModelPages::new(
        sink.clone(),
        NonZeroUsize::MIN.saturating_add(1),
        Arc::new(Ids(AtomicU64::new(30))),
    );
    let reference = pages
        .register(candidate.pages)
        .map_err(|e| format!("{e:?}"))?;
    let other = pages
        .register(changed.pages)
        .map_err(|e| format!("{e:?}"))?;
    assert_ne!(reference, other);
    let other_rows = pages
        .rows(&VizRowsRequest {
            model_ref: other.clone(),
            result_revision: id(2),
            start: 0,
            count: 1,
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(other_rows.rows[0].display_name, "different label");
    pages.release_model(&other).map_err(|e| format!("{e:?}"))?;
    let rows_request = VizRowsRequest {
        model_ref: reference.clone(),
        result_revision: id(2),
        start: 0,
        count: 2,
    };
    let rows = pages.rows(&rows_request).map_err(|e| format!("{e:?}"))?;
    assert_eq!(rows.rows[0].group_ref.as_deref(), Some("group-0"));
    assert_eq!(
        rows.rows[0].quantity.as_ref().ok_or("quantity")?.unit(),
        "items"
    );
    let group = pages
        .group_rows(&reference, "group-0", 0, 5)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        group.rows[0].item_ref.as_ref().ok_or("item")?.item_no,
        candidate.model.histogram.bins[0].members[0]
    );
    assert_eq!(
        group.rows[0].size.amount,
        SizeAmount::Exact {
            bytes: 5_u64.into()
        }
    );
    assert_eq!(
        group.group_total.amount,
        SizeAmount::Exact {
            bytes: 5_u64.into()
        }
    );
    assert_eq!(
        pages
            .group_rows_cancellable(&reference, "group-0", 0, 5, &mut || true)
            .err(),
        Some(Error::Cancelled)
    );
    pages
        .release_model(&reference)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        pages.group_rows(&reference, "group-0", 0, 1).err(),
        Some(Error::ModelExpired)
    );
    let second = histogram::prepare(request(), &mut || false).map_err(|e| format!("{e:?}"))?;
    let current = pages.register(second.pages).map_err(|e| format!("{e:?}"))?;
    let _new = publish(&sink, 3)?;
    assert_eq!(
        pages.group_rows(&current, "group-0", 0, 1).err(),
        Some(Error::StaleVersion)
    );
    let stale = histogram::prepare(request(), &mut || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(pages.register(stale.pages).err(), Some(Error::StaleVersion));
    Ok(())
}
