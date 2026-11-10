use super::*;

pub(super) fn scan_harness_with_gate(
    finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
) -> Result<ScanHarness, Box<dyn std::error::Error>> {
    scan_harness_with_comparison(finish_gate, None)
}

pub(super) fn scan_harness_with_comparison(
    finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    comparison: Option<Arc<dyn crate::scope::exclusion::PathComparison>>,
) -> Result<ScanHarness, Box<dyn std::error::Error>> {
    scan_harness_with_options(finish_gate, comparison, None)
}

pub(super) fn scan_harness_with_options(
    finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    comparison: Option<Arc<dyn crate::scope::exclusion::PathComparison>>,
    limit: Option<crate::desktop::SharedTableLimitSource>,
) -> Result<ScanHarness, Box<dyn std::error::Error>> {
    let ids = Arc::new(Ids {
        next: AtomicUsize::new(1),
        fallback: Id::parse("018f0000-0000-7000-8000-000000000000")?,
    });
    let (release, blocked) = mpsc::channel();
    let (send, terminal) = mpsc::channel();
    let roots = Arc::new(Roots {
        file_id: AtomicUsize::new(1),
        opens: AtomicUsize::new(0),
        reads: Arc::new(AtomicUsize::new(0)),
        release: Arc::new(Mutex::new(blocked)),
        finish_gate,
    });
    let clock = Arc::new(Clock);
    let runtime = Arc::new(JobRuntime::new_with_limits(
        ids.clone(),
        clock.clone(),
        Arc::new(History),
        Arc::new(Events(send)),
        FlowLimits {
            metadata_io: 2,
            queued_items: 4,
            item_bytes: 1024,
        },
    )?);
    let mut builder = EngineBuilder::default();
    if let Some(comparison) = comparison.clone() {
        crate::scope::register_review_with_comparison(
            &mut builder,
            roots.clone(),
            clock,
            comparison,
        );
    } else {
        crate::scope::register_review(&mut builder, roots.clone(), clock);
    }
    let workers = Arc::new(crate::workers::Workers::start(runtime.clone())?);
    let versions = match limit {
        Some(limit) => {
            crate::scan::register_with_limit(&mut builder, workers, roots.clone(), ids, limit)?
        }
        None => crate::scan::register_with_comparison(
            &mut builder,
            workers,
            roots.clone(),
            ids,
            comparison,
        )?,
    };
    crate::jobs::register_with_scan_store(&mut builder, runtime, versions.clone());
    Ok(ScanHarness {
        engine: builder.build()?,
        roots,
        release,
        terminal,
        versions,
    })
}
