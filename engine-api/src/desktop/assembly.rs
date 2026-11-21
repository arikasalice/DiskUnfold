//! Register the desktop analysis use cases with injected OS and storage ports.
use std::{
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};

use du_core_contracts::{
    Id,
    job::{HistoryChanged, JobHistory},
    ports::Random,
    scan_observation::ScopeRootPort,
    scope_observation_log::ScopeObservationLog,
    volume::VolumePort,
};
use du_job_runtime::{
    FlowLimits, JobClock, JobEvent, JobEvents, JobIdentity, JobLaneStatusEvent, JobProgressEvent,
    JobRuntime,
};

use super::{
    CompletedScanSource, DesktopEvents, RecentScopeAdmissions, ScanDiagnosticsFactory,
    SharedTableLimit, SharedTableLimitSource, VisualizationServices, recent_scope_admissions,
    scan_completion, shutdown, utc_now_ms,
};
use crate::size_rule::SizeRules;
use crate::{api::EngineBuilder, jobs, scan, scope, targets, workers::Workers};

// Provisional until the target probe deadline is measured across supported devices.
const TARGET_PROBE_DEADLINE: Duration = Duration::from_secs(2);
const TARGET_ENUMERATION_DEADLINE: Duration = Duration::from_secs(2);

/// Preserves the sample terminal tap and sends lane status through the desktop scan relay.
struct DesktopJobEvents {
    sample: Arc<crate::sample::SampleEvents>,
    scans: Arc<scan_completion::ScanEvents>,
}

impl JobEvents for DesktopJobEvents {
    fn publish(&self, event: JobEvent) {
        self.sample.publish(event);
    }

    fn publish_history_changed(&self, event: HistoryChanged) {
        self.sample.publish_history_changed(event);
    }

    fn publish_progress(&self, event: JobProgressEvent) {
        self.sample.publish_progress(event);
    }

    fn publish_lane_status(&self, event: JobLaneStatusEvent) {
        self.scans.publish_lane_status(event);
    }

    fn retire_job(&self, id: &Id) {
        self.sample.retire_job(id);
    }
}

/// Registers the desktop analysis flow and returns its event source.
///
/// Every desktop platform uses this one registration. It registers the
/// `scan.metadata` job kind together with `scan.start` and `scan.failures`;
/// registering it twice on one builder makes the engine build fail.
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
            size_rules: None,
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
    scan_events: Arc<scan_completion::ScanEvents>,
    runtime: Arc<JobRuntime>,
}

impl DesktopServices {
    /// Registers retention commands using this desktop runtime's clock.
    ///
    /// The host supplies its current policy source, owner saver, read-only
    /// history adapter and session undo-info port. Retention remains GUI-only.
    pub fn register_history_retention<R>(
        &self,
        builder: &mut EngineBuilder,
        policy_source: crate::history::retention::HistoryRetentionPolicySource,
        policy_saver: crate::history::retention::HistoryRetentionPolicySaver,
        expiry_reader: R,
        undo_info: Arc<dyn du_core_contracts::job::SessionUndoInfo>,
    ) where
        R: du_core_contracts::job::HistoryExpiryRead + 'static,
    {
        crate::history::retention::register(
            builder,
            self.runtime.clone(),
            policy_source,
            policy_saver,
            expiry_reader,
            undo_info,
        );
    }

    /// Binds the log that stores one observation of the scope totals per finished GUI scan.
    ///
    /// Bind it once, before the first scan starts. The log is written after the scan's
    /// terminal history record, in its own transaction, and a failure there is reported
    /// as a diagnostic without changing the scan. A desktop without a bound log records
    /// nothing.
    pub fn bind_scope_observation_log(
        &self,
        log: Arc<dyn ScopeObservationLog>,
    ) -> Result<(), &'static str> {
        self.scan_events.bind_observation_log(log)
    }
}

struct DesktopRegistrationOptions {
    shared_table_limit: SharedTableLimitSource,
    size_rules: Option<SizeRules>,
    diagnostics: Option<ScanDiagnosticsFactory>,
    recent_admission_capacity: Option<NonZeroUsize>,
}

/// Sources the composition root injects into the desktop analysis flow.
pub struct DesktopSources {
    /// Setting snapshot read when a scan is admitted.
    pub shared_table_limit: SharedTableLimitSource,
    /// The size rule registry, assembled once, and the source naming the rule ID.
    pub size_rules: SizeRules,
    /// Capacity of the deferred recent-scope worker handoff; `None` disables it.
    pub recent_admission_capacity: Option<NonZeroUsize>,
}

/// Registers analysis with every injected source, including the size rule selection.
///
/// The composition root assembles `sources.size_rules` once, for example with
/// [`SizeRules::default_bundle`], and stops startup when that fails. Each
/// `scan.start` reads the rule source once and pins the latest revision of its ID.
pub fn register_services_with_sources(
    builder: &mut EngineBuilder,
    roots: Arc<dyn ScopeRootPort>,
    volumes: Arc<dyn VolumePort>,
    history: Arc<dyn JobHistory>,
    random: &dyn Random,
    sources: DesktopSources,
) -> Result<DesktopServices, Box<dyn std::error::Error>> {
    register_services_inner(
        builder,
        roots,
        volumes,
        history,
        random,
        DesktopRegistrationOptions {
            shared_table_limit: sources.shared_table_limit,
            size_rules: Some(sources.size_rules),
            diagnostics: None,
            recent_admission_capacity: sources.recent_admission_capacity,
        },
    )
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
            size_rules: None,
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
            size_rules: None,
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
        size_rules,
        diagnostics,
        recent_admission_capacity,
    } = options;
    let ids = Arc::new(SystemIdentity);
    let clock = Arc::new(SystemClock(Instant::now()));
    let events = Arc::new(DesktopEvents::default());
    let scan_events = Arc::new(scan_completion::ScanEvents::new(events.clone()));
    let sample_events = Arc::new(crate::sample::SampleEvents::new(scan_events.clone()));
    let runtime_events = Arc::new(DesktopJobEvents {
        sample: sample_events.clone(),
        scans: scan_events.clone(),
    });
    let runtime = Arc::new(JobRuntime::new_with_limits(
        ids.clone(),
        clock.clone(),
        history,
        runtime_events,
        FlowLimits {
            metadata_io: 16,
            queued_items: 64,
            item_bytes: 256 * 1024,
        },
    )?);
    let workers = Arc::new(Workers::start(runtime.clone())?);
    builder.on_shutdown_hooks(shutdown::hooks(
        runtime.clone(),
        workers.clone(),
        scan_events.clone(),
    ));
    scope::register_review_with_volumes(builder, roots.clone(), volumes.clone(), clock.clone());
    targets::register_with_probe(
        builder,
        volumes.clone(),
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
    let sets = scan::register_with_options(
        builder,
        workers.clone(),
        roots,
        ids.clone(),
        scan::ScanRegistrationOptions {
            comparison: None,
            volumes: Some(volumes),
            limit: shared_table_limit,
            diagnostics,
            admissions: recent_publisher,
            priority_control: true,
            size_rules,
        },
    )?;
    let store = sets.source();
    let groups = crate::group::compose(&store, &runtime, &workers, ids.clone());
    let completed_scans = scan_events.bind(store.clone())?;
    super::sample_sink::bind(
        &sample_events,
        builder,
        &workers,
        store.clone(),
        ids.clone(),
        &events,
    )?;
    let visualization = VisualizationServices::new(store.clone(), ids.clone(), workers.clone());
    let search = crate::browse::folder::commands::register(builder, sets, ids, workers, random)?;
    crate::scan_failures::register(builder, store.clone(), random);
    let path_copy = crate::browse::copy_path::PathCopy(store.clone());
    jobs::register_with_scan_store(builder, runtime.clone(), store);
    Ok(DesktopServices {
        events,
        path_copy,
        search,
        visualization,
        completed_scans,
        recent_scope_admissions,
        groups,
        scan_events,
        runtime,
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
