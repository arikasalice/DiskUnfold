use du_analysis_index::{
    IngestSink, Observation, ResultVersion,
    visualization::{
        bundle,
        geometry::{Rect, Scale},
        histogram::Basis,
        histogram_dispatch, registry,
        typed_layout::{Dispatcher, Strategy},
    },
};
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
    size_units::{SI_UNITS, UNIT_TABLE_REVISION},
    visualization::rows::VizRowsRequest,
};
use du_engine_api::viz_pages::{
    ModelPages,
    histogram::{self, BinPresentation, PinnedRendered},
    production::RowPresentation,
};
use du_job_runtime::JobIdentity;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
struct Ids(AtomicU8);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes([self.0.fetch_add(1, Ordering::SeqCst); 16])
    }
}
fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}
fn publish(sink: &IngestSink, rev: u8) -> Result<Arc<ResultVersion>, String> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut b = sink
        .open_forest(id(1), vec![root])
        .map_err(|e| format!("{e:?}"))?;
    b.add_directory_at(0, vec![])
        .map_err(|e| format!("{e:?}"))?;
    for (name, n) in [(b"a".as_slice(), 5_u64), (b"b".as_slice(), 1000)] {
        b.add_file_at(
            0,
            Observation {
                path: vec![name.to_vec()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact { bytes: n.into() },
                }),
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    }
    b.seal(id(rev), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))
}
fn dispatcher() -> Result<Dispatcher, String> {
    let mut b = registry::Builder::<Box<dyn Strategy>>::default();
    bundle::register_histogram_candidate(&mut b).map_err(|e| format!("{e:?}"))?;
    Ok(Dispatcher::new(b.build(&[]).map_err(|e| format!("{e:?}"))?))
}
fn bounds() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 100,
    }
}
#[test]
fn selected_registered_geometry_binds_to_pages_and_rejects_replaced_source() -> Result<(), String> {
    let sink = IngestSink::default();
    let version = publish(&sink, 2)?;
    let scope = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let d = dispatcher()?;
    let source_request = || histogram_dispatch::Request {
        version: &version,
        scope: &scope,
        size_basis: SizeBasis::Logical,
        unit_table: &SI_UNITS,
        unit_table_revision: UNIT_TABLE_REVISION,
        bounds: bounds(),
        basis: Basis::ItemCount,
        scale: Scale::Automatic,
        unknown_reason: "fixture.unknown",
    };
    assert!(matches!(
        PinnedRendered::compute(version.clone(), source_request(), &d, &mut || true),
        Err(histogram::Error::Cancelled)
    ));
    let selected = PinnedRendered::compute(version.clone(), source_request(), &d, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let member_ptr = selected.rendered().prepared().model().histogram.bins[1]
        .members
        .as_ptr();
    let bins = selected.rendered().prepared().unit_bins().bins.clone();
    let axis = selected.rendered().selected().axis_max;
    let compute = || {
        PinnedRendered::compute(version.clone(), source_request(), &d, &mut || false)
            .map_err(|e| format!("{e:?}"))
    };
    let request = || histogram::Request {
        version: version.clone(),
        scope: &scope,
        size_basis: SizeBasis::Logical,
        bins: &bins,
        bounds: bounds(),
        scale: Scale::Automatic,
        fingerprint: "fixture".into(),
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
                    result_version: id(2),
                    item_no: n,
                },
                legend_class_key: "c".into(),
                display_flags: vec![],
            })
            .collect(),
    };
    assert!(matches!(
        histogram::prepare_selected(request(), compute()?, &mut || true),
        Err(histogram::Error::Cancelled)
    ));
    let foreign_sink = IngestSink::default();
    let foreign = publish(&foreign_sink, 2)?;
    assert!(matches!(
        PinnedRendered::compute(foreign.clone(), source_request(), &d, &mut || false),
        Err(histogram::Error::Source)
    ));
    let mut foreign_request = request();
    foreign_request.version = foreign;
    assert!(matches!(
        histogram::prepare_selected(foreign_request, compute()?, &mut || false),
        Err(histogram::Error::Source)
    ));
    check_wrong_scale(request(), compute()?)?;
    let candidate = histogram::prepare_selected(request(), selected, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        candidate.model().histogram.bins[1].members.as_ptr(),
        member_ptr
    );
    assert_eq!(candidate.model().histogram.axis_max, axis);
    assert_eq!(candidate.pages().rows.len(), bins.len());
    assert_eq!(candidate.pages().groups["g0"].rows.len(), 1);
    assert_eq!(candidate.pages().groups["g1"].rows.len(), 1);
    let pages = ModelPages::new(
        sink.clone(),
        NonZeroUsize::MIN.saturating_add(3),
        Arc::new(Ids(AtomicU8::new(10))),
    );
    let cancelled_candidate = histogram::prepare_selected(request(), compute()?, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let mid_cancel_candidate = histogram::prepare_selected(request(), compute()?, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    check_cancellation(&pages, cancelled_candidate, mid_cancel_candidate)?;
    let mut deferred = pages
        .defer_histogram_pages(candidate, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        pages
            .histogram_frame(deferred.reserved_reference())
            .is_err()
    );
    let model_ref = pages
        .commit_histogram_pages(&mut deferred, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let repeated = histogram::prepare_selected(request(), compute()?, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let mut repeated = pages
        .defer_histogram_pages(repeated, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        pages
            .commit_histogram_pages(&mut repeated, &mut || false)
            .map_err(|e| format!("{e:?}"))?,
        model_ref
    );
    assert!(
        pages
            .commit_histogram_pages(&mut deferred, &mut || false)
            .is_err()
    );
    check_published(&pages, &model_ref, &bins, axis)?;
    let stale = histogram::prepare_selected(request(), compute()?, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let mut deferred_stale = pages
        .defer_histogram_pages(stale, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    let stale_reference = deferred_stale.reserved_reference().to_owned();
    let _next = publish(&sink, 3)?;
    assert!(
        pages
            .commit_histogram_pages(&mut deferred_stale, &mut || false)
            .is_err()
    );
    check_absent(&pages, &stale_reference);
    check_absent(&pages, &model_ref);
    Ok(())
}

fn check_wrong_scale(
    mut request: histogram::Request<'_>,
    selected: PinnedRendered,
) -> Result<(), String> {
    request.scale = Scale::Fixed(10);
    assert!(matches!(
        histogram::prepare_selected(request, selected, &mut || false),
        Err(histogram::Error::Source)
    ));
    Ok(())
}

mod bytes;
mod f24;

fn check_cancellation(
    pages: &ModelPages,
    cancelled_candidate: histogram::SelectedCandidate,
    mid_cancel_candidate: histogram::SelectedCandidate,
) -> Result<(), String> {
    let mut deferred = pages
        .defer_histogram_pages(cancelled_candidate, &mut || false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        pages.commit_histogram_pages(&mut deferred, &mut || true),
        Err(du_engine_api::viz_pages::Error::Cancelled)
    ));
    assert!(
        pages
            .histogram_frame(deferred.reserved_reference())
            .is_err()
    );
    assert!(
        pages
            .rows(&VizRowsRequest {
                model_ref: deferred.reserved_reference().into(),
                result_revision: id(2),
                start: 0,
                count: 1
            })
            .is_err()
    );
    let mut checks = 0;
    assert!(matches!(
        pages.defer_histogram_pages(mid_cancel_candidate, &mut || {
            checks += 1;
            checks > 9
        }),
        Err(histogram::Error::Cancelled)
    ));
    Ok(())
}

fn check_published(
    pages: &ModelPages,
    model_ref: &str,
    bins: &[du_analysis_index::visualization::histogram::Bin],
    axis: Option<u64>,
) -> Result<(), String> {
    let frame = pages
        .histogram_frame(model_ref)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(frame.status, histogram::HistogramStatus::Ready);
    assert_eq!(frame.axis_max, axis);
    assert_eq!(frame.unit_bins.bins, bins);
    assert_eq!(frame.bins[0].group_ref, "g0");
    assert!(frame.bins[0].rect.is_some());
    let rows = pages
        .rows(&VizRowsRequest {
            model_ref: model_ref.to_owned(),
            result_revision: id(2),
            start: 0,
            count: 1,
        })
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(rows.rows[0].group_ref.as_deref(), Some("g0"));
    Ok(())
}
fn check_absent(pages: &ModelPages, reference: &str) {
    assert!(pages.histogram_frame(reference).is_err());
    assert!(
        pages
            .rows(&VizRowsRequest {
                model_ref: reference.into(),
                result_revision: id(2),
                start: 0,
                count: 1
            })
            .is_err()
    );
}
