//! Register the desktop analysis use cases with injected OS and storage ports.
use std::{
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};

use du_core_contracts::{
    Id, job::JobHistory, ports::Random, scan_observation::ScopeRootPort, volume::VolumePort,
};
use du_job_runtime::{FlowLimits, JobClock, JobIdentity, JobRuntime};

use super::{
    CompletedScanSource, DesktopEvents, RecentScopeAdmissions, ScanDiagnosticsFactory,
    SharedTableLimit, SharedTableLimitSource, VisualizationServices, recent_scope_admissions,
    scan_completion, shutdown, utc_now_ms,
};
use crate::{api::EngineBuilder, jobs, scan, scope, targets, workers::Workers};

// Provisional until the target probe deadline is measured across supported devices.
const TARGET_PROBE_DEADLINE: Duration = Duration::from_secs(2);
const TARGET_ENUMERATION_DEADLINE: Duration = Duration::from_secs(2);

/// Registers the Windows desktop analysis flow and returns its event source.
pub fn register(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
) -> Result<Arc<DesktopEvents>, Box<dyn std::error::Error>> {
    register_with_shared_table_limit(
        builder,
        roots,
        volumes,
        history,
        random,
        Arc::new(SharedTableLimit::default),
    )
}

/// Registers the desktop flow with a source of validated settings for future scans.
///
/// The source is read at each scan-start attempt; an admitted job retains its value.
pub fn register_with_shared_table_limit(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    shared_table_limit: SharedTableLimitSource,
) -> Result<Arc<DesktopEvents>, Box<dyn std::error::Error>> {
    Ok(register_services(builder, roots, volumes, history, random, shared_table_limit)?.events)
}

/// Registers the desktop flow with an opt-in, per-scan diagnostic factory.
/// This is a Rust composition hook; it does not expose diagnostics over IPC.
pub fn register_with_scan_diagnostics(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    diagnostics: ScanDiagnosticsFactory,
) -> Result<Arc<DesktopEvents>, Box<dyn std::error::Error>> {
    Ok(register_services_inner(
        builder,
        roots,
        volumes,
        history,
        random,
        DesktopRegistrationOptions {
            shared_table_limit: Arc::new(SharedTableLimit::default),
            diagnostics: Some(diagnostics),
            recent_admission_capacity: None,
        },
    )?
    .events)
}

/// Desktop resources used to compose local app-host commands and events.
pub struct DesktopServices {
    pub events: Arc<DesktopEvents>,
    pub path_copy: crate::browse::copy_path::PathCopy,
    /// Internal Rust search entry point sharing the GUI query regions.
    pub search: Option<Arc<crate::search::SearchEntry>>,
    /// Internal composition handle tied to this desktop's scan store and job workers.
    pub visualization: VisualizationServices,
    /// Internal comparison admission tied to the exact completed desktop result.
    pub completed_scans: CompletedScanSource,
    /// Internal app-host worker queue. Only newly admitted GUI scans are sent.
    pub recent_scope_admissions: Option<RecentScopeAdmissions>,
    /// F36 group use cases; app-host registers them with its rule store.
    pub groups: crate::group::GroupAssembly,
}

struct DesktopRegistrationOptions {
    shared_table_limit: SharedTableLimitSource,
    diagnostics: Option<ScanDiagnosticsFactory>,
    recent_admission_capacity: Option<NonZeroUsize>,
}

/// Registers analysis and exposes the observed-path reader for app-host only.
pub fn register_services(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    shared_table_limit: SharedTableLimitSource,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    register_services_inner(
        builder,
        roots,
        volumes,
        history,
        random,
        DesktopRegistrationOptions {
            shared_table_limit,
            diagnostics: None,
            recent_admission_capacity: None,
        },
    )
}

/// Registers analysis with the opt-in deferred recent-scope worker handoff.
///
/// App-host confirms delivery of each new scan receipt before its storage worker
/// can observe that admission. The scan job may still be running.
pub fn register_services_with_recent_scope_worker(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    shared_table_limit: SharedTableLimitSource,
    recent_admission_capacity: NonZeroUsize,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    register_services_inner(
        builder,
        roots,
        volumes,
        history,
        random,
        DesktopRegistrationOptions {
            shared_table_limit,
            diagnostics: None,
            recent_admission_capacity: Some(recent_admission_capacity),
        },
    )
}

fn register_services_inner(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    options: DesktopRegistrationOptions,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    let DesktopRegistrationOptions {
        shared_table_limit,
        diagnostics,
        recent_admission_capacity,
    } = options;
    let ids = Arc::new(SystemIdentity);
    let clock = Arc::new(SystemClock(Instant::now()));
    let events = Arc::new(DesktopEvents::default());
    let scan_events = Arc::new(scan_completion::ScanEvents::new(events.clone()));
    let runtime = Arc::new(JobRuntime::new_with_limits(
        ids.clone(),
        clock.clone(),
        history,
        scan_events.clone(),
        FlowLimits {
            metadata_io: 16,
            queued_items: 64,
            item_bytes: 256 * 1024,
        },
    )?);
    let workers = Arc::new(Workers::start(runtime.clone())?);
    builder.on_shutdown_hooks(shutdown::hooks(runtime.clone(), workers.clone()));
    scope::register_review(builder, roots.clone(), clock.clone());
    targets::register_with_probe(
        builder,
        volumes,
        clock,
        workers.clone(),
        targets::Deadlines {
            enumeration: TARGET_ENUMERATION_DEADLINE,
            probe: TARGET_PROBE_DEADLINE,
        },
    )?;
    let (recent_publisher, recent_scope_admissions) =
        if let Some(capacity) = recent_admission_capacity {
            let (publisher, admissions) = recent_scope_admissions(capacity.get());
            (Some(publisher), Some(admissions))
        } else {
            (None, None)
        };
    let sets = match recent_publisher {
        Some(publisher) => scan::register_with_admissions(
            builder,
            workers.clone(),
            roots,
            ids.clone(),
            shared_table_limit,
            diagnostics,
            publisher,
        )?,
        None => scan::register_with_limit(
            builder,
            workers.clone(),
            roots,
            ids.clone(),
            shared_table_limit,
            diagnostics,
        )?,
    };
    let store = sets.source();
    let groups = crate::group::compose(&store, &runtime, &workers, ids.clone());
    let completed_scans = scan_events.bind(store.clone())?;
    let visualization = VisualizationServices::new(store.clone(), ids.clone(), workers.clone());
    let search = crate::browse::folder::commands::register(builder, sets, ids, workers, random)?;
    crate::scan_failures::register(builder, store.clone(), random);
    let path_copy = crate::browse::copy_path::PathCopy(store.clone());
    jobs::register_with_scan_store(builder, runtime, store);
    Ok(DesktopServices {
        events,
        path_copy,
        search,
        visualization,
        completed_scans,
        recent_scope_admissions,
        groups,
    })
}

struct SystemIdentity;

impl JobIdentity for SystemIdentity {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes(*uuid::Uuid::now_v7().as_bytes())
    }
}

struct SystemClock(Instant);

impl JobClock for SystemClock {
    fn monotonic_now(&self) -> Option<Duration> {
        Some(self.0.elapsed())
    }

    fn now_ms(&self) -> i64 {
        utc_now_ms()
    }
}
