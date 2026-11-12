use super::*;
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ResultEnvelope,
    browse_page_ipc::QueryStrategy,
    job::{HistoryAppend, JobHistory, JobState, JobSummary},
    ports::{PortError, Random},
    result::ResultItem,
    visualization::{
        model::{VizMatchKey, VizStaleness, VizStalenessState, VizTableRevisions},
        request::VizRenderRequest,
        response::VizRenderResponse,
    },
};
use du_engine_api::{
    api::{CallerCtx, Channel, Engine, EngineBuilder},
    error::EngineError,
    viz_runtime::{
        Assembly, EvidenceProducer, ItemEvidence, LegendClass, Runtime, SourceFacts,
        SourcePolicyFacts, ViewPolicy,
    },
};
use du_job_runtime::{JobClock, JobEvent, JobEvents, JobRuntime};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
struct Clock;
impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        0
    }
}
struct History;
impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Err("visualization must not write history".into())
    }
}
struct Events(mpsc::Sender<JobEvent>);
impl JobEvents for Events {
    fn publish(&self, event: JobEvent) {
        let _ = self.0.send(event);
    }
}
struct Entropy;
impl Random for Entropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(5);
        Ok(())
    }
}
struct Producer {
    block: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
    unit_iec: AtomicBool,
    required_data_mode: AtomicU8,
    histogram_bins_absent: AtomicBool,
    hierarchy_unknown: AtomicBool,
    child_fails: AtomicBool,
}
impl EvidenceProducer for Producer {
    fn required_data_facts(
        &self,
        source: &current_analysis::CurrentAnalysis,
        request: &VizRenderRequest,
    ) -> Result<Option<du_engine_api::required_data::FactValues>, EngineError> {
        use du_engine_api::required_data::Evidence;
        let mode = self.required_data_mode.load(Ordering::Relaxed);
        if mode == 0 {
            return Ok(None);
        }
        let parent = request.aggregate_scope_ref.item_no;
        let children: Vec<_> = source
            .version()
            .items()
            .iter()
            .filter(|item| item.parent == Some(parent))
            .collect();
        let folder = !children.is_empty();
        let sizes = !children.is_empty() && children.iter().all(|item| item.size.is_some());
        Ok(Some(du_engine_api::required_data::FactValues {
            folder_hierarchy: if self.hierarchy_unknown.load(Ordering::Relaxed) {
                Evidence::Unknown
            } else if folder {
                Evidence::Known(())
            } else {
                Evidence::Absent
            },
            item_sizes: if sizes {
                Evidence::Known(())
            } else {
                Evidence::Absent
            },
            numeric_values: if sizes {
                Evidence::Known(())
            } else {
                Evidence::Absent
            },
            trend_observations: Evidence::Unknown,
            comparison: Evidence::Unknown,
            histogram_bins: if self.histogram_bins_absent.load(Ordering::Relaxed) {
                Evidence::Absent
            } else if self.histogram_facts(source, request)?.is_some() {
                Evidence::Known(())
            } else {
                Evidence::Unknown
            },
            attribution: Evidence::Unknown,
        }))
    }
    fn source_policy_facts(
        &self,
        _: &current_analysis::CurrentAnalysis,
        request: &VizRenderRequest,
    ) -> Result<Option<SourcePolicyFacts>, EngineError> {
        Ok(source_selection::owner_policy(request))
    }
    fn ranked_other(
        &self,
        _: &current_analysis::CurrentAnalysis,
        _: &VizRenderRequest,
    ) -> Result<Option<du_engine_api::viz_pages::ranked_group::OtherPresentation>, EngineError>
    {
        Ok(Some(
            du_engine_api::viz_pages::ranked_group::OtherPresentation {
                group_ref: "fixture.other".into(),
                display_name: "Other".into(),
                legend_class_key: "fixture.class".into(),
                display_flags: vec![],
            },
        ))
    }
    fn histogram_facts(
        &self,
        _: &current_analysis::CurrentAnalysis,
        _: &VizRenderRequest,
    ) -> Result<Option<du_engine_api::viz_runtime::HistogramFacts>, EngineError> {
        if self.histogram_bins_absent.load(Ordering::Relaxed) {
            return Err(EngineError::new(
                "fixture.histogram_must_not_be_consulted",
                vec![],
            ));
        }
        Ok(Some(histogram_runtime::facts(self)))
    }
    fn source_facts(
        &self,
        source: &current_analysis::CurrentAnalysis,
        request: &VizRenderRequest,
    ) -> Result<SourceFacts, EngineError> {
        if request.classification_criterion == "fixture.owner_rule" {
            return Err(EngineError::new("fixture.legacy_called", vec![]));
        }
        let applied = source.version().size_rule();
        let mut size_rule = QueryStrategy {
            id: applied.id.into(),
            revision: applied.revision,
        };
        match request.classification_criterion.as_str() {
            "fixture.bad_rule_id" => size_rule.id = "fixture.other_rule".into(),
            "fixture.bad_rule_revision" => size_rule.revision += 1,
            _ => {}
        }
        Ok(SourceFacts {
            staleness: VizStaleness {
                state: VizStalenessState::Current,
                reason_key: None,
            },
            tables: VizTableRevisions {
                interpretation: 1,
                required_data: 1,
                legend: 1,
                view_policy: 1,
                size_rule,
            },
            available_formats: vec![],
            classes: HashMap::from([(
                "fixture.class".into(),
                LegendClass {
                    label_key: "fixture.label".into(),
                    condition: None,
                },
            )]),
        })
    }
    fn child(
        &self,
        _: &current_analysis::CurrentAnalysis,
        _: &ResultItem,
        _: &VizRenderRequest,
    ) -> Result<ItemEvidence, EngineError> {
        if self.child_fails.load(Ordering::Relaxed) {
            return Err(EngineError::new("fixture.evidence_unavailable", vec![]));
        }
        let blocking = self
            .block
            .lock()
            .map_err(|_| EngineError::new("support.internal_error", vec![]))?
            .take();
        if let Some((entered, release)) = blocking {
            entered
                .send(())
                .map_err(|_| EngineError::new("support.internal_error", vec![]))?;
            release
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| EngineError::new("support.internal_error", vec![]))?;
        }
        Ok(ItemEvidence {
            presentation: ChildPolicyEvidence {
                path_compare_key: None,
                legend_class_key: "fixture.class".into(),
                display_flags: vec![],
            },
            match_key: VizMatchKey::Unknown {
                reason_key: "fixture.key_unavailable".into(),
            },
        })
    }
}
struct Fixture {
    engine: Engine,
    jobs: Arc<JobRuntime>,
    sink: IngestSink,
    events: mpsc::Receiver<JobEvent>,
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    session: Arc<Session>,
    producer: Arc<Producer>,
}
impl Fixture {
    fn new(sizes: [u64; 2]) -> Result<Self, String> {
        Self::with_wait(sizes, Duration::ZERO)
    }
    fn with_wait(sizes: [u64; 2], synchronous_wait: Duration) -> Result<Self, String> {
        Self::with_policy(sizes, synchronous_wait, NonZeroUsize::MIN.saturating_add(3))
    }
    fn with_policy(
        sizes: [u64; 2],
        synchronous_wait: Duration,
        max_work: NonZeroUsize,
    ) -> Result<Self, String> {
        Self::with_tree(sizes, synchronous_wait, max_work, false)
    }
    fn with_tree(
        sizes: [u64; 2],
        synchronous_wait: Duration,
        max_work: NonZeroUsize,
        nested: bool,
    ) -> Result<Self, String> {
        let sink = IngestSink::default();
        if nested {
            super::locate::publish_hierarchy(&sink, id(3))?;
        } else {
            publish_sizes(&sink, id(2), id(3), sizes)?;
        }
        Self::from_sink(sink, synchronous_wait, max_work)
    }
    fn from_sink(
        sink: IngestSink,
        synchronous_wait: Duration,
        max_work: NonZeroUsize,
    ) -> Result<Self, String> {
        histogram_strategy::fixture(
            sink,
            synchronous_wait,
            max_work,
            histogram_runtime::dispatcher()?,
        )
    }
    fn call(&self, name: &str, body: Value) -> ResultEnvelope {
        self.engine.call(
            CallEnvelope {
                name: name.into(),
                contract_version: ENVELOPE_VERSION,
                correlation_id: id(50),
                body,
            },
            &CallerCtx::new(Channel::Gui, id(50), None),
        )
    }
    fn open(&self) -> Result<Value, String> {
        Ok(self.ok("viz.view.open", json!({}))?["viewChannel"].clone())
    }
    fn ok(&self, name: &str, body: Value) -> Result<Value, String> {
        let answer = self.call(name, body);
        if let Some(error) = answer.error {
            return Err(format!("{name}: {error:?}"));
        }
        Ok(answer.body)
    }
    fn render(&self, view: &Value, seq: u32) -> Result<Value, String> {
        self.ok("viz.render", render_body(view, seq))
    }
    fn detach(&self, view: &Value, seq: u32) -> Result<Value, String> {
        self.ok(
            "viz.cancel",
            json!({"target":{"kind":"subscription","viewChannel":view,"requestSeq":seq}}),
        )
    }
    fn finished(&self, job: &Id, state: JobState) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(3);
        while let Ok(event) = self
            .events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            if let JobEvent::Finished {
                summary,
                history_recorded,
                ..
            } = event
                && &summary.job_id == job
            {
                assert_eq!(summary.state, state);
                assert!(!history_recorded);
                assert!(summary.result_version_id.is_none());
                return Ok(());
            }
        }
        Err("terminal F24 event missing".into())
    }
}
fn render_body(view: &Value, seq: u32) -> Value {
    let item = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    json!({"viewChannel":view,"requestSeq":seq,"format":"treemap","resultRevision":id(3),"aggregateScopeRef":item,"navigationScopeRef":item,"measure":"size","sizeBasis":"logical","classificationCriterion":"fixture.criterion","colorCriterion":"fixture.color","denominatorBasis":"scope","viewport":{"width":800,"height":600,"scale":1.0},"visibleWindow":{"left":0,"top":0,"right":1000000,"bottom":1000000},"minAreaPx2":16,"depthLimit":1})
}
fn job(response: &Value) -> Result<Id, String> {
    assert_eq!(response["kind"], "job");
    serde_json::from_value(response["jobRef"].clone()).map_err(|e| e.to_string())
}
#[test]
fn registered_render_shares_work_replaces_sequence_and_reads_committed_model() -> Result<(), String>
{
    let fixture = Fixture::new([25, 75])?;
    let a = fixture.open()?;
    let b = fixture.open()?;
    let first = job(&fixture.render(&a, 1)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    assert_eq!(job(&fixture.render(&a, 2)?)?, first);
    assert_eq!(job(&fixture.render(&b, 1)?)?, first);
    let mut invalid = render_body(&a, 3);
    invalid["viewport"]["scale"] = json!(99.0);
    assert_eq!(
        fixture
            .call("viz.render", invalid)
            .error
            .ok_or("invalid viewport accepted")?
            .reason_key,
        "viz.viewport_invalid"
    );
    assert_eq!(fixture.detach(&a, 1)?["controlResult"], "no_effect");
    assert_eq!(fixture.detach(&a, 2)?["controlResult"], "accepted");
    assert_eq!(
        fixture.jobs.state(&first).map_err(|e| e.to_string())?,
        JobState::Running
    );
    fixture.release.send(()).map_err(|e| e.to_string())?;
    fixture.finished(&first, JobState::Done)?;
    let model = fixture.render(&b, 2)?;
    assert_eq!(model["kind"], "model");
    let parsed: VizRenderResponse =
        serde_json::from_value(model.clone()).map_err(|e| e.to_string())?;
    parsed.validate()?;
    let reference = &model["model"]["modelRef"];
    assert_eq!(fixture.render(&b, 3)?["model"]["modelRef"], *reference);
    assert_eq!(
        fixture.ok(
            "viz.rows",
            json!({"modelRef":reference,"resultRevision":id(3),"start":0,"count":10})
        )?["totalCount"],
        2
    );
    assert_eq!(fixture.detach(&b, 3)?["controlResult"], "already_terminal");
    Ok(())
}
#[test]
fn direct_cancel_stops_all_members_and_cancelling_work_rejects_new_members() -> Result<(), String> {
    let fixture = Fixture::new([25, 75])?;
    let a = fixture.open()?;
    let b = fixture.open()?;
    let c = fixture.open()?;
    let physical = job(&fixture.render(&a, 1)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    assert_eq!(job(&fixture.render(&b, 1)?)?, physical);
    let cancel = json!({"target":{"kind":"job","jobRef":physical}});
    assert_eq!(
        fixture.ok("viz.cancel", cancel.clone())?["controlResult"],
        "accepted"
    );
    assert_eq!(
        fixture.ok("viz.cancel", cancel.clone())?["controlResult"],
        "no_effect"
    );
    assert_eq!(
        fixture
            .call("viz.render", render_body(&c, 1))
            .error
            .ok_or("joined cancelling job")?
            .reason_key,
        "viz.busy"
    );
    fixture.release.send(()).map_err(|e| e.to_string())?;
    fixture.finished(&physical, JobState::Cancelled)?;
    assert_eq!(
        fixture.ok("viz.cancel", cancel)?["controlResult"],
        "already_terminal"
    );
    let retried = fixture.render(&c, 2)?;
    if retried["kind"] == "job" {
        assert_ne!(job(&retried)?, physical);
    } else {
        assert_eq!(retried["kind"], "model");
    }
    Ok(())
}
#[test]
fn valid_different_condition_retires_only_the_replaced_channel() -> Result<(), String> {
    let fixture = Fixture::new([25, 75])?;
    let a = fixture.open()?;
    let b = fixture.open()?;
    let original = job(&fixture.render(&a, 1)?)?;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    assert_eq!(job(&fixture.render(&b, 1)?)?, original);
    let mut changed = render_body(&a, 2);
    changed["labelMode"] = json!("name_only");
    let replacement = fixture.ok("viz.render", changed.clone())?;
    if replacement["kind"] == "job" {
        let job = job(&replacement)?;
        assert_ne!(job, original);
        fixture.finished(&job, JobState::Done)?;
    } else {
        assert_eq!(replacement["kind"], "model");
    }
    assert_eq!(fixture.detach(&a, 1)?["controlResult"], "no_effect");
    assert_eq!(
        fixture.jobs.state(&original).map_err(|e| e.to_string())?,
        JobState::Running
    );
    fixture.release.send(()).map_err(|e| e.to_string())?;
    fixture.finished(&original, JobState::Done)?;
    let old_model = fixture.render(&b, 2)?;
    changed["requestSeq"] = json!(3);
    let new_model = fixture.ok("viz.render", changed)?;
    assert_eq!(new_model["kind"], "model");
    assert_ne!(
        old_model["model"]["modelRef"],
        new_model["model"]["modelRef"]
    );
    assert_ne!(
        old_model["model"]["conditionFingerprint"],
        new_model["model"]["conditionFingerprint"]
    );
    Ok(())
}
mod fallback_location;
mod hierarchy_tests;
mod histogram_runtime;
mod replacement;
mod source_selection;

mod group_rows_dispatch;
mod histogram_strategy;
mod ranked_bar_runtime;
mod ranked_group_runtime;

mod required_data_runtime;

mod retirement_runtime;

mod fast_completion_runtime;

mod source_replacement_runtime;
