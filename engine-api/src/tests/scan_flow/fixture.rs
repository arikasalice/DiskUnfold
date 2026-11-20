use super::*;

pub(super) fn scan_harness_with_gate(
    finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
) -> Result<ScanHarness, Box<dyn std::error::Error>> {
    scan_harness_with_admissions(finish_gate, None, None, false, false, None, false)
}

pub(super) fn scan_harness_with_priority() -> Result<ScanHarness, Box<dyn std::error::Error>> {
    scan_harness_with_admissions(None, None, None, false, true, None, false)
}

pub(super) fn scan_harness_with_recent_admissions()
-> Result<ScanHarness, Box<dyn std::error::Error>> {
    scan_harness_with_admissions(None, None, None, true, false, None, false)
}

pub(super) fn scan_harness_with_comparison(
    finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    comparison: Option<Arc<dyn crate::scope::exclusion::PathComparison>>,
) -> Result<ScanHarness, Box<dyn std::error::Error>> {
    scan_harness_with_admissions(finish_gate, comparison, None, false, false, None, false)
}

pub(super) fn scan_harness_with_options(
    finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    comparison: Option<Arc<dyn crate::scope::exclusion::PathComparison>>,
    limit: Option<crate::desktop::SharedTableLimitSource>,
) -> Result<ScanHarness, Box<dyn std::error::Error>> {
    scan_harness_with_admissions(finish_gate, comparison, limit, false, false, None, false)
}

pub(super) fn scan_harness_with_size_rule_source(
    source: crate::size_rule::SizeRuleSource,
) -> Result<ScanHarness, Box<dyn std::error::Error>> {
    let rules = crate::size_rule::SizeRules::default_bundle(source)?;
    scan_harness_with_size_rules(rules)
}

pub(super) fn scan_harness_with_size_rules(
    rules: crate::size_rule::SizeRules,
) -> Result<ScanHarness, Box<dyn std::error::Error>> {
    scan_harness_with_admissions(None, None, None, false, false, Some(rules), false)
}

/// A harness that also admits command-line calls, through a filter that passes
/// every response unchanged.
pub(super) fn scan_harness_with_cli() -> Result<ScanHarness, Box<dyn std::error::Error>> {
    scan_harness_with_admissions(None, None, None, false, false, None, true)
}

struct PassThrough;

impl crate::api::OutboundFilter for PassThrough {
    fn filter(
        &self,
        _: &str,
        _: &CallerCtx,
        response: serde_json::Value,
    ) -> Result<serde_json::Value, EngineError> {
        Ok(response)
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_harness_with_admissions(
    finish_gate: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    comparison: Option<Arc<dyn crate::scope::exclusion::PathComparison>>,
    limit: Option<crate::desktop::SharedTableLimitSource>,
    recent: bool,
    priority_scenario: bool,
    size_rules: Option<crate::size_rule::SizeRules>,
    cli: bool,
) -> Result<ScanHarness, Box<dyn std::error::Error>> {
    let ids = Arc::new(Ids {
        next: AtomicUsize::new(1),
        fallback: Id::parse("018f0000-0000-7000-8000-000000000000")?,
    });
    let (release, blocked) = mpsc::channel();
    let (send, terminal) = mpsc::channel();
    let (
        priority,
        priority_opened,
        priority_visit_entered,
        priority_visit_release,
        priority_b_completed,
        priority_visits,
    ) = if priority_scenario {
        let (open_entered, open_entered_rx) = mpsc::channel();
        let (visit_entered, visit_entered_rx) = mpsc::channel();
        let (visit_release, visit_released) = mpsc::channel();
        let (visits, visits_rx) = mpsc::channel();
        let (completed_b, completed_b_rx) = mpsc::channel();
        (
            Some(Arc::new(super::PriorityState {
                open_entered,
                visit_entered,
                visit_release: Arc::new(Mutex::new(visit_released)),
                visits,
                completed_b,
            })),
            Some(open_entered_rx),
            Some(visit_entered_rx),
            Some(visit_release),
            Some(completed_b_rx),
            Some(visits_rx),
        )
    } else {
        (None, None, None, None, None, None)
    };
    let roots = Arc::new(Roots {
        file_id: AtomicUsize::new(1),
        opens: AtomicUsize::new(0),
        reads: Arc::new(AtomicUsize::new(0)),
        recent_observations: Arc::new(AtomicUsize::new(0)),
        recent_opens: Arc::new(AtomicUsize::new(0)),
        recent_live_handles: Arc::new(AtomicUsize::new(0)),
        recent_unavailable: std::sync::atomic::AtomicBool::new(false),
        read_handles: Arc::new(Mutex::new(Vec::new())),
        release: Arc::new(Mutex::new(blocked)),
        finish_gate,
        priority,
    });
    let clock = Arc::new(Clock);
    let events = Arc::new(crate::desktop::scan_completion::ScanEvents::new(Arc::new(
        Events(send),
    )));
    let runtime = Arc::new(JobRuntime::new_with_limits(
        ids.clone(),
        clock.clone(),
        Arc::new(History),
        events.clone(),
        FlowLimits {
            // The priority scenario runs one worker. That worker stays in the gated open
            // of area A, so area B stays queued until the test releases the scan. A second
            // worker could open and finish B first, and a request for B then has no effect.
            metadata_io: if priority_scenario { 1 } else { 2 },
            queued_items: 4,
            item_bytes: 1024,
        },
    )?);
    let mut builder = EngineBuilder::default();
    if cli {
        builder.with_outbound_filter(PassThrough);
    }
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
    let (recent_publisher, recent_scope_admissions) = if recent {
        let (publisher, admissions) = crate::desktop::recent_scope_admissions(1);
        (Some(publisher), Some(admissions))
    } else {
        (None, None)
    };
    let versions = match (limit, recent_publisher) {
        (None, None) if size_rules.is_some() => crate::scan::register_with_options(
            &mut builder,
            workers,
            roots.clone(),
            ids,
            crate::scan::ScanRegistrationOptions {
                comparison: None,
                limit: Arc::new(du_core_contracts::settings::SharedTableLimit::default),
                diagnostics: None,
                admissions: None,
                priority_control: false,
                size_rules,
            },
        )?
        .source(),
        (Some(limit), None) => crate::scan::register_with_limit(
            &mut builder,
            workers,
            roots.clone(),
            ids,
            limit,
            None,
        )?
        .source(),
        (None, Some(publisher)) => crate::scan::register_with_admissions(
            &mut builder,
            workers,
            roots.clone(),
            ids,
            Arc::new(du_core_contracts::settings::SharedTableLimit::default),
            None,
            publisher,
        )?
        .source(),
        (None, None) if priority_scenario => {
            crate::scan::register_with_priority(&mut builder, workers, roots.clone(), ids)?
        }
        (None, None) => crate::scan::register_with_comparison(
            &mut builder,
            workers,
            roots.clone(),
            ids,
            comparison,
        )?,
        (Some(_), Some(_)) => {
            return Err("test harness does not combine recent admission and custom setting".into());
        }
    };
    let completed_scans = events.bind(versions.clone())?;
    crate::jobs::register_with_scan_store(&mut builder, runtime, versions.clone());
    Ok(ScanHarness {
        engine: builder.build()?,
        roots,
        release,
        terminal,
        versions,
        completed_scans,
        scan_events: events,
        recent_scope_admissions,
        priority_opened,
        priority_visit_entered,
        priority_visit_release,
        priority_b_completed,
        priority_visits,
    })
}
