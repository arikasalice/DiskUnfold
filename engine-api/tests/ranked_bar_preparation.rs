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

struct Ids;
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes([9; 16])
    }
}
struct Alternate(Arc<AtomicUsize>);
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
    let mut build = sink
        .open(Id::from_v7_bytes([1; 16]))
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
    let session = Session::new(
        sink,
        NonZeroUsize::MIN,
        Arc::new(Ids),
        Dispatcher::new(builder.build(&[]).map_err(|e| format!("{e:?}"))?),
    );
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
    let mut missing = descriptor.clone();
    missing.strategy.revision = 99;
    assert!(
        session
            .compute_ranked_bar(&source, request(), &missing, &mut || false)
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        session
            .compute_ranked_bar(&source, request(), &descriptor, &mut || true)
            .is_err()
    );
    Ok(())
}
