//! Registered F06 runtime for the admitted live, unfiltered single-parent path.
//!
//! Desktop activation requires source selection, classification, correspondence and
//! view-policy owners. Their typed evidence is injected; this module does not infer it.
use crate::{
    api::EngineBuilder,
    current_analysis::CurrentAnalysis,
    error::EngineError,
    query_region::{Owner, RegionRegistry},
    viz_pages::{
        render::{CommittedRender, DeferredRender},
        session::{ChildPolicyEvidence, Session},
    },
    workers::Workers,
};
use du_analysis_index::IngestSink;
use du_core_contracts::{
    Id,
    ports::Random,
    result::{ResultItem, ResultVersionRead},
    visualization::{
        model::{VizMatchKey, VizModel, VizStaleness, VizTableRevisions},
        request::VizRenderRequest,
        response::VizRenderResponse,
    },
};
use du_job_runtime::{JobIdentity, JobRuntime};
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
    sync::{Arc, Mutex},
    time::Duration,
};
mod admission;
mod commands;
mod compute;
mod control;
mod envelope;
mod histogram;
mod histogram_model;
mod lifecycle;
mod ranked_bar;
mod ranked_group_model;
mod ranked_projection;
pub mod selection;

/// Histogram policy and bin presentation frozen by the composition owner at admission.
#[derive(Clone)]
pub struct HistogramFacts {
    /// Unit definitions selected for this admitted calculation.
    pub unit_table: &'static du_core_contracts::size_units::UnitTable,
    /// Revision assigned by the unit-table owner.
    pub unit_table_revision: u32,
    /// Model-local group references and presentation for each unit interval.
    pub bins: Vec<crate::viz_pages::histogram::BinPresentation>,
    /// Owner-provided reason for an unknown byte quantity.
    pub unknown_reason: String,
}

/// Frozen policy limits supplied from the registered view policy.
#[derive(Clone)]
pub struct ViewPolicy {
    pub revision: u32,
    pub max_scale: f64,
    pub max_shapes: usize,
    pub max_channels: NonZeroUsize,
    pub max_work: NonZeroUsize,
    pub synchronous_wait: Duration,
}
/// Legend label and canonical filter supplied by the classification owner.
#[derive(Clone)]
pub struct LegendClass {
    pub label_key: String,
    pub condition: Option<du_core_contracts::browse_open_ipc::QueryCondition>,
}
/// Immutable facts retained for the admitted result and policy revisions.
#[derive(Clone)]
pub struct SourceFacts {
    pub staleness: VizStaleness,
    pub tables: VizTableRevisions,
    pub classes: HashMap<String, LegendClass>,
    /// Formats the required-data owner evaluated for this same pinned source.
    pub available_formats: Vec<String>,
}
/// Item evidence supplied by the pinned path and presentation owners.
pub struct ItemEvidence {
    pub presentation: ChildPolicyEvidence,
    pub match_key: VizMatchKey,
}
/// Trusted composition dependency, never a wire callback or caller-supplied flag.
/// Implementations must return evidence for the supplied immutable result, and
/// preserve semantics for the lifetime of their registered policy revisions.
pub trait EvidenceProducer: Send + Sync + 'static {
    /// Supplies model-local Other presentation for the pinned ranked-bar request.
    fn ranked_other(
        &self,
        _source: &CurrentAnalysis,
        _request: &VizRenderRequest,
    ) -> Result<Option<crate::viz_pages::ranked_group::OtherPresentation>, EngineError> {
        Ok(None)
    }
    /// Supplies immutable byte-histogram policy for this exact admitted source.
    fn histogram_facts(
        &self,
        _source: &CurrentAnalysis,
        _request: &VizRenderRequest,
    ) -> Result<Option<HistogramFacts>, EngineError> {
        Ok(None)
    }
    fn source_facts(
        &self,
        source: &CurrentAnalysis,
        request: &VizRenderRequest,
    ) -> Result<SourceFacts, EngineError>;
    fn child(
        &self,
        source: &CurrentAnalysis,
        item: &ResultItem,
        request: &VizRenderRequest,
    ) -> Result<ItemEvidence, EngineError>;
}
/// Dependencies owned by one desktop engine/window session.
pub struct Assembly {
    pub source: IngestSink,
    pub session: Arc<Session>,
    pub jobs: Arc<JobRuntime>,
    pub ids: Arc<dyn JobIdentity>,
    pub producer: Arc<dyn EvidenceProducer>,
    pub policy: ViewPolicy,
}
struct ChannelState {
    sequence: u32,
    active: Option<Arc<Work>>,
}
#[derive(Default)]
struct State {
    closed: bool,
    channels: HashMap<Id, ChannelState>,
    work: HashMap<String, Arc<Work>>,
}
struct Work {
    key: String,
    job: Mutex<Option<Id>>,
    members: Mutex<HashSet<Id>>,
    computed: Mutex<Computed>,
}
#[derive(Default)]
struct Computed {
    committed: Option<RuntimeCommitted>,
    wire_model: Option<Arc<VizModel>>,
    histogram_revision: Option<Id>,
    ranked_group: Option<(Id, String, u32)>,
}
enum RuntimeCommitted {
    Projection(CommittedRender),
    Histogram {
        fallback: du_core_contracts::visualization::render::VizFallback,
        no_attribution: bool,
    },
    RankedGroup {
        fallback: du_core_contracts::visualization::render::VizFallback,
        group_ref: String,
        row_index: u32,
    },
}
impl Clone for RuntimeCommitted {
    fn clone(&self) -> Self {
        match self {
            Self::Projection(value) => Self::Projection(value.clone()),
            Self::Histogram {
                fallback,
                no_attribution,
            } => Self::Histogram {
                fallback: fallback.clone(),
                no_attribution: *no_attribution,
            },
            Self::RankedGroup {
                fallback,
                group_ref,
                row_index,
            } => Self::RankedGroup {
                fallback: fallback.clone(),
                group_ref: group_ref.clone(),
                row_index: *row_index,
            },
        }
    }
}
enum Candidate {
    Projection(DeferredRender),
    Histogram(crate::viz_pages::histogram::DeferredSelectedPages),
    RankedGroup(crate::viz_pages::ranked_group::DeferredRankedGroup),
}
type CandidateSlot = Mutex<Option<Candidate>>;
type RankedWork = (Candidate, Option<VizModel>, u64, Option<(String, u32)>);
#[derive(Clone)]
struct Admitted {
    request: VizRenderRequest,
    source: CurrentAnalysis,
    facts: SourceFacts,
    key: String,
    descriptor: du_core_contracts::visualization::LayoutDescriptor,
    max_shapes: usize,
    histogram: Option<HistogramFacts>,
    ranked_other: Option<crate::viz_pages::ranked_group::OtherPresentation>,
}
/// Owns admission, shared subscriptions and the physical F24 publication boundary.
/// Lock order: admission serial -> F24 gate -> members -> computed -> cache/source.
/// Submission admission only writes the reserved job ID under F24's table lock.
/// Publication never acquires the admission serial or reenters the runtime.
pub struct Runtime {
    assembly: Assembly,
    selection: Option<Arc<dyn selection::WindowScanSelection>>,
    regions: RegionRegistry<()>,
    owner: Owner,
    serial: Mutex<()>,
    state: Mutex<State>,
    workers: Arc<Workers>,
}
impl Runtime {
    /// Registers real Engine use cases; do not also register standalone ViewChannels.
    pub fn register(
        builder: &mut EngineBuilder,
        assembly: Assembly,
        random: &dyn Random,
    ) -> Result<Arc<Self>, &'static str> {
        Self::register_inner(builder, assembly, random, None, None)
    }

    /// Registers the opt-in runtime whose source is bound to an owning window's selection.
    ///
    /// Requests are rejected when the provider has no selected scan or when any
    /// supplied source reference or result revision disagrees with that selection.
    pub fn register_with_selection(
        builder: &mut EngineBuilder,
        assembly: Assembly,
        random: &dyn Random,
        selection: Arc<dyn selection::WindowScanSelection>,
    ) -> Result<Arc<Self>, &'static str> {
        Self::register_inner(builder, assembly, random, Some(selection), None)
    }

    /// Registers visualization on the analysis desktop's existing worker pool.
    pub(crate) fn register_on_desktop(
        builder: &mut EngineBuilder,
        assembly: Assembly,
        random: &dyn Random,
        selection: Arc<dyn selection::WindowScanSelection>,
        workers: Arc<Workers>,
    ) -> Result<Arc<Self>, &'static str> {
        Self::register_inner(builder, assembly, random, Some(selection), Some(workers))
    }

    fn register_inner(
        builder: &mut EngineBuilder,
        assembly: Assembly,
        random: &dyn Random,
        selection: Option<Arc<dyn selection::WindowScanSelection>>,
        workers: Option<Arc<Workers>>,
    ) -> Result<Arc<Self>, &'static str> {
        if assembly.policy.revision == 0
            || !assembly.policy.max_scale.is_finite()
            || assembly.policy.max_scale < 1.0
            || assembly.policy.max_shapes == 0
        {
            return Err("invalid visualization policy");
        }
        let owner = Owner {
            session: assembly.ids.next_id(),
            window: assembly.ids.next_id(),
        };
        let regions = RegionRegistry::new(
            owner.clone(),
            assembly.policy.max_channels.get(),
            assembly.ids.clone(),
            random,
        )
        .map_err(|_| "view channel initialization failed")?;
        assembly
            .jobs
            .register(compute::descriptor())
            .map_err(|_| "visualization job registration failed")?;
        let workers = match workers {
            Some(workers) => workers,
            None => Arc::new(
                Workers::start(assembly.jobs.clone())
                    .map_err(|_| "visualization worker initialization failed")?,
            ),
        };
        let runtime = Arc::new(Self {
            assembly,
            selection,
            regions,
            owner,
            serial: Mutex::new(()),
            state: Mutex::new(State::default()),
            workers,
        });
        commands::register(builder, runtime.clone());
        crate::viz_pages::commands::register_rows(builder, runtime.assembly.session.clone());
        crate::viz_pages::commands::register_group_rows(builder, runtime.assembly.session.clone());
        crate::viz_pages::commands::register_locate(builder, runtime.assembly.session.clone());
        let retiring = runtime.clone();
        builder.on_session_retire(move || {
            let _ = retiring.retire();
        });
        Ok(runtime)
    }
}
fn internal() -> EngineError {
    EngineError::new("support.internal_error", vec![])
}
fn invalid() -> EngineError {
    EngineError::new("command.body_invalid", vec![])
}
fn unavailable() -> EngineError {
    EngineError::new("ref.unavailable", vec![])
}
fn runtime_error(error: du_job_runtime::RuntimeError) -> EngineError {
    EngineError::new(
        match error {
            du_job_runtime::RuntimeError::QueueFull
            | du_job_runtime::RuntimeError::DeliveryBusy => "viz.busy",
            du_job_runtime::RuntimeError::NotFound => "job.not_found",
            _ => "support.internal_error",
        },
        vec![],
    )
}
