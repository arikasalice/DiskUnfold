use du_analysis_index::{
    IngestSink, Observation,
    visualization::{
        bar_projection::ChildrenRequest,
        bundle,
        geometry::{Rect, Scale},
        registry,
        typed_layout::{self, BarError, BarInput, Dispatcher, Strategy},
    },
};
use du_core_contracts::{
    Id, ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::ResultCompleteness,
    visualization::{LayoutDescriptor, LayoutStrategyKey},
};
use du_engine_api::viz_pages::session::Session;
use du_job_runtime::JobIdentity;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
mod ranked_bar_f24;

#[derive(Default)]
struct Ids(AtomicUsize);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes([9 + self.0.fetch_add(1, Ordering::SeqCst) as u8; 16])
    }
}
struct Alternate(Arc<AtomicUsize>);
struct Corrupt;
impl Strategy for Corrupt {
    fn bar(
        &self,
        input: BarInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<du_analysis_index::visualization::bar_projection::Ranking, BarError> {
        let mut result = Strategy::bar(&typed_layout::RankedBar, input, cancelled)?;
        if let Some(bar) = result.bars.last_mut()
            && let du_analysis_index::visualization::bar_projection::BarIdentity::Other {
                size, ..
            } = &mut bar.identity
        {
            size.amount = SizeAmount::Exact {
                bytes: 51_u64.into(),
            };
            bar.value = du_analysis_index::visualization::geometry::Weight::Exact(51);
        }
        Ok(result)
    }
}
impl Strategy for Alternate {
    fn bar(
        &self,
        input: BarInput<'_>,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<du_analysis_index::visualization::bar_projection::Ranking, BarError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Strategy::bar(&typed_layout::RankedBar, input, cancelled)
    }
}

#[test]
fn session_prepares_pinned_rows_before_selected_ranked_strategy() -> Result<(), String> {
    let sink = IngestSink::default();
    let root = du_core_contracts::result::ResultScopeRoot {
        path: du_core_contracts::result::ResultNativePath {
            encoding: du_core_contracts::PathEncoding::Unix,
            raw: du_core_contracts::PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_forest(Id::from_v7_bytes([1; 16]), vec![root.clone()])
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for name in [b"a", b"b"] {
        build
            .add_file(Observation {
                path: vec![name.to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: 25_u64.into(),
                    },
                }),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let source = build
        .seal(Id::from_v7_bytes([2; 16]), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_ranked_bar(&mut builder).map_err(|e| format!("{e:?}"))?;
    let descriptor = LayoutDescriptor {
        format: "bar".into(),
        strategy: LayoutStrategyKey {
            id: "viz.bar".into(),
            revision: 2,
        },
    };
    builder
        .register(descriptor.clone(), Box::new(Alternate(calls.clone())))
        .map_err(|e| format!("{e:?}"))?;
    let mut corrupt_descriptor = descriptor.clone();
    corrupt_descriptor.strategy.revision = 3;
    builder
        .register(corrupt_descriptor.clone(), Box::new(Corrupt))
        .map_err(|e| format!("{e:?}"))?;
    let session = Arc::new(Session::new(
        sink.clone(),
        NonZeroUsize::MIN.saturating_add(2),
        Arc::new(Ids::default()),
        Dispatcher::new(builder.build(&[]).map_err(|e| format!("{e:?}"))?),
    ));
    let scope = ItemRef {
        result_version: Id::from_v7_bytes([2; 16]),
        item_no: 0,
    };
    let request = || ChildrenRequest {
        version: &source,
        scope: &scope,
        basis: SizeBasis::Logical,
        bounds: Rect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        scale: Scale::Automatic,
        top: 2,
        max_bars: 2,
    };
    let selected = session
        .compute_ranked_bar(&source, request(), &descriptor, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(selected.descriptor, descriptor);
    assert_eq!(selected.output.result_version, scope.result_version);
    assert_eq!(selected.output.rows.len(), 2);
    assert_eq!(selected.output.bars.len(), 2);
    assert!(
        selected
            .output
            .bars
            .iter()
            .all(|bar| bar.geometry.is_some())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut limited = request();
    limited.top = 0;
    limited.max_bars = 1;
    let grouped = session
        .compute_pinned_ranked_bar(&source, limited, &descriptor, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(grouped.selected().output.bars.len(), 1);
    match &grouped.selected().output.bars[0].identity {
        du_analysis_index::visualization::bar_projection::BarIdentity::Other { members, size } => {
            assert_eq!(members.len(), 2);
            assert_eq!(
                size.amount,
                SizeAmount::Exact {
                    bytes: 50_u64.into()
                }
            );
        }
        _ => return Err("expected F03 verified Other".into()),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let candidate = du_engine_api::viz_pages::ranked_group::Request {
        pinned: grouped,
        scope: &scope,
        basis: SizeBasis::Logical,
        presentations: (1..=2)
            .map(
                |item_no| du_engine_api::viz_pages::production::RowPresentation {
                    item: ItemRef {
                        result_version: scope.result_version.clone(),
                        item_no,
                    },
                    legend_class_key: "fixture.class".into(),
                    display_flags: vec![],
                },
            )
            .collect(),
        other: du_engine_api::viz_pages::ranked_group::OtherPresentation {
            group_ref: "fixture.other".into(),
            display_name: "Other".into(),
            legend_class_key: "fixture.class".into(),
            display_flags: vec![],
        },
        fingerprint: "fixture".into(),
        max_shapes: 1,
    };
    let pending = session
        .defer_ranked_group(candidate, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    verify_published_group(&session, &descriptor, &scope, pending)?;
    assert!(make_pending(&session, &session, &source, &scope, &corrupt_descriptor, 0).is_err());
    let mut missing = descriptor.clone();
    missing.strategy.revision = 99;
    assert!(
        session
            .compute_ranked_bar(&source, request(), &missing, &mut || false)
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(
        session
            .compute_ranked_bar(&source, request(), &descriptor, &mut || true)
            .is_err()
    );
    let mut foreign_builder = registry::Builder::<Box<dyn Strategy>>::default();
    foreign_builder
        .register(
            descriptor.clone(),
            Box::new(Alternate(Arc::new(AtomicUsize::new(0)))),
        )
        .map_err(|e| format!("{e:?}"))?;
    let foreign = Session::new(
        sink.clone(),
        NonZeroUsize::MIN,
        Arc::new(Ids::default()),
        Dispatcher::new(foreign_builder.build(&[]).map_err(|e| format!("{e:?}"))?),
    );
    assert!(make_pending(&session, &foreign, &source, &scope, &descriptor, 0).is_err());
    verify_other_outcomes(&session, &source, &scope, &descriptor, &sink, root)?;
    Ok(())
}

fn make_pending(
    session: &Session,
    target: &Session,
    source: &Arc<du_analysis_index::ResultVersion>,
    scope: &ItemRef,
    descriptor: &LayoutDescriptor,
    top: usize,
) -> Result<du_engine_api::viz_pages::ranked_group::DeferredRankedGroup, String> {
    let pinned = session
        .compute_pinned_ranked_bar(
            source,
            ChildrenRequest {
                version: source,
                scope,
                basis: SizeBasis::Logical,
                bounds: Rect {
                    left: 0,
                    top: 0,
                    right: 100,
                    bottom: 100,
                },
                scale: Scale::Automatic,
                top,
                max_bars: top + 1,
            },
            descriptor,
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    let input = du_engine_api::viz_pages::ranked_group::Request {
        pinned,
        scope,
        basis: SizeBasis::Logical,
        presentations: (1..=2)
            .map(
                |item_no| du_engine_api::viz_pages::production::RowPresentation {
                    item: ItemRef {
                        result_version: scope.result_version.clone(),
                        item_no,
                    },
                    legend_class_key: "fixture.class".into(),
                    display_flags: vec![],
                },
            )
            .collect(),
        other: du_engine_api::viz_pages::ranked_group::OtherPresentation {
            group_ref: "fixture.other".into(),
            display_name: "Other".into(),
            legend_class_key: "fixture.class".into(),
            display_flags: vec![],
        },
        fingerprint: "fixture".into(),
        max_shapes: top + 1,
    };
    target
        .defer_ranked_group(input, &mut || false)
        .map_err(|e| format!("{e:?}"))
}

fn verify_published_group(
    session: &Arc<Session>,
    descriptor: &LayoutDescriptor,
    scope: &ItemRef,
    pending: du_engine_api::viz_pages::ranked_group::DeferredRankedGroup,
) -> Result<(), String> {
    let private_ref = pending.reserved_reference().to_owned();
    assert!(session.ranked_frame(&private_ref).is_err());
    assert!(
        session
            .group_rows(&private_ref, "fixture.other", 0, 10)
            .is_err()
    );
    let model_ref = ranked_bar_f24::publish(session.clone(), pending)?;
    let frame = session
        .ranked_frame(&model_ref)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(&frame.descriptor, descriptor);
    assert_eq!(frame.shapes.len(), 1);
    assert!(frame.shapes[0].geometry.is_some());
    let members = session
        .group_rows(&model_ref, "fixture.other", 0, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(members.result_revision, scope.result_version);
    assert_eq!(members.rows.len(), 2);
    assert_eq!(
        members.group_total.amount,
        SizeAmount::Exact {
            bytes: 50_u64.into()
        }
    );
    let rows = session
        .rows(&du_core_contracts::visualization::rows::VizRowsRequest {
            model_ref,
            result_revision: scope.result_version.clone(),
            start: 0,
            count: 10,
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0].group_ref.as_deref(), Some("fixture.other"));
    Ok(())
}

fn verify_other_outcomes(
    session: &Arc<Session>,
    source: &Arc<du_analysis_index::ResultVersion>,
    scope: &ItemRef,
    descriptor: &LayoutDescriptor,
    sink: &IngestSink,
    root: du_core_contracts::result::ResultScopeRoot,
) -> Result<(), String> {
    let mixed = make_pending(session, session, source, scope, descriptor, 1)?;
    let mixed_ref = ranked_bar_f24::publish(session.clone(), mixed)?;
    let mixed_rows = session
        .rows(&du_core_contracts::visualization::rows::VizRowsRequest {
            model_ref: mixed_ref.clone(),
            result_revision: scope.result_version.clone(),
            start: 0,
            count: 10,
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(mixed_rows.rows.len(), 2);
    assert!(mixed_rows.rows[0].item_ref.is_some());
    assert_eq!(
        mixed_rows.rows[1].group_ref.as_deref(),
        Some("fixture.other")
    );
    assert_eq!(
        session
            .group_rows(&mixed_ref, "fixture.other", 0, 10)
            .map_err(|e| format!("{e:?}"))?
            .rows
            .len(),
        1
    );
    let cancelled = make_pending(session, session, source, scope, descriptor, 0)?;
    let cancelled_ref = ranked_bar_f24::execute(
        session.clone(),
        cancelled,
        true,
        du_core_contracts::job::JobState::Cancelled,
    )?;
    assert!(session.ranked_frame(&cancelled_ref).is_err());
    assert!(
        session
            .group_rows(&cancelled_ref, "fixture.other", 0, 10)
            .is_err()
    );
    let stale = make_pending(session, session, source, scope, descriptor, 0)?;
    let mut replacement = sink
        .open_forest(Id::from_v7_bytes([3; 16]), vec![root])
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(Id::from_v7_bytes([4; 16]), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(session.ranked_frame(&mixed_ref).is_err());
    assert!(
        session
            .group_rows(&mixed_ref, "fixture.other", 0, 10)
            .is_err()
    );
    let stale_ref = ranked_bar_f24::execute(
        session.clone(),
        stale,
        false,
        du_core_contracts::job::JobState::Failed,
    )?;
    assert!(session.ranked_frame(&stale_ref).is_err());
    assert!(
        session
            .group_rows(&stale_ref, "fixture.other", 0, 10)
            .is_err()
    );
    Ok(())
}
