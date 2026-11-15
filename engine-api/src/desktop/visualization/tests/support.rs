//! Composition and call helpers for the default visualization registration tests.
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use du_analysis_index::{IngestSink, Observation};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id, PathBytes, PathEncoding, ResultEnvelope, SizeAmount,
    SizeBasis, SizeEvidence, SizeValue,
    job::{HistoryAppend, JobHistory, JobSummary},
    ports::{PortError, PortErrorKind, Random},
    result::{ResultCompleteness, ResultItem, ResultNativePath, ResultScopeRoot},
    scan_observation::{OpenedScopeRoot, ScopeRootPort},
    size_units::{IEC_UNITS, SI_UNITS},
    visualization::request::VizRenderRequest,
    volume::{
        Capability, CapabilityState, VolumeCapabilities, VolumeCapacity, VolumeEnumeration,
        VolumePort,
    },
};
use serde_json::{Value, json};

use super::super::{DefaultVisualization, RuntimeParts};
use crate::{
    api::{CallerCtx, Channel, Engine, EngineBuilder},
    current_analysis::CurrentAnalysis,
    desktop::SharedTableLimit,
    error::EngineError,
    required_data::{Attribution, Evidence, FactValues},
    viz_production::ProductionEvidence,
    viz_runtime::{EvidenceProducer, HistogramFacts, ItemEvidence, SourcePolicyFacts, ViewPolicy},
};

/// Inert platform ports; the tests publish their results straight into the store.
pub(super) struct Ports;

impl Random for Ports {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(7);
        Ok(())
    }
}

impl JobHistory for Ports {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}

impl ScopeRootPort for Ports {
    fn open_root(&self, _: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        Err(unsupported())
    }
}

impl VolumePort for Ports {
    fn enumerate(&self) -> VolumeEnumeration {
        let none = Capability {
            state: CapabilityState::Unsupported,
            reason_key: Some("os.unsupported"),
        };
        VolumeEnumeration {
            mounts: Ok(Vec::new()),
            capabilities: VolumeCapabilities {
                enumerate: none.clone(),
                identity: none.clone(),
                capacity: none.clone(),
                path_access: none.clone(),
                path_compare_rule: none.clone(),
                mount_boundary: none.clone(),
                alias: none,
            },
        }
    }

    fn path_access(&self, _: &[u8]) -> Result<(), PortError> {
        Err(unsupported())
    }

    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        Err(unsupported())
    }
}

fn unsupported() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "os.unsupported".into(),
        os_code: None,
    }
}

pub(super) fn id(value: u8) -> Id {
    Id::from_v7_bytes([value; 16])
}

pub(super) fn view_policy(max_shapes: usize) -> ViewPolicy {
    ViewPolicy {
        revision: 1,
        max_scale: 4.0,
        max_shapes,
        max_channels: NonZeroUsize::MIN.saturating_add(7),
        max_work: NonZeroUsize::MIN.saturating_add(7),
        synchronous_wait: Duration::from_secs(20),
    }
}

/// An engine composed through the desktop assembly, plus its result store.
pub(super) struct Harness {
    pub(super) engine: Engine,
    pub(super) sink: IngestSink,
    /// Selects IEC instead of SI for histogram requests.
    pub(super) iec: Arc<AtomicBool>,
    /// Children the producer was asked about.
    pub(super) children_read: Arc<AtomicUsize>,
}

type Produced = (Arc<dyn EvidenceProducer>, Arc<AtomicUsize>);

impl Harness {
    /// Registers the product visualization the way the app-host assembly does.
    pub(super) fn product(max_shapes: usize) -> Result<Self, String> {
        Self::compose(max_shapes, |_| None)
    }

    /// Registers the default runtime with a producer that claims data no source has.
    pub(super) fn overclaiming(max_shapes: usize) -> Result<Self, String> {
        Self::compose(max_shapes, |product| {
            let counter = Arc::new(AtomicUsize::new(0));
            let producer: Arc<dyn EvidenceProducer> = Arc::new(Overclaiming {
                inner: product,
                children_read: counter.clone(),
            });
            Some((producer, counter))
        })
    }

    fn compose(
        max_shapes: usize,
        wrap: impl FnOnce(ProductionEvidence) -> Option<Produced>,
    ) -> Result<Self, String> {
        let fail = |error: Box<dyn std::error::Error>| error.to_string();
        let mut builder = EngineBuilder::default();
        let desktop = crate::desktop::register_services(
            &mut builder,
            Arc::new(Ports),
            Arc::new(Ports),
            Arc::new(Ports),
            &Ports,
            Arc::new(SharedTableLimit::default),
        )
        .map_err(fail)?;
        let iec = Arc::new(AtomicBool::new(false));
        let selected = iec.clone();
        let unit_table: crate::viz_production::UnitTableSource = Arc::new(move || {
            if selected.load(Ordering::SeqCst) {
                &IEC_UNITS
            } else {
                &SI_UNITS
            }
        });
        let options = DefaultVisualization {
            policy: view_policy(max_shapes),
            model_capacity: NonZeroUsize::MIN.saturating_add(15),
            unit_table: unit_table.clone(),
        };
        let mut children_read = Arc::new(AtomicUsize::new(0));
        let services = &desktop.visualization;
        let formats = [
            "treemap",
            "bar",
            "trend",
            "waterfall",
            "histogram",
            "sankey",
        ]
        .map(str::to_owned)
        .to_vec();
        let product = ProductionEvidence::new(formats, 1, unit_table);
        match wrap(product) {
            None => services
                .register_default(&mut builder, &Ports, options)
                .map_err(str::to_owned)?,
            Some((producer, counter)) => {
                children_read = counter;
                let dispatcher = du_analysis_index::visualization::bundle::default_dispatcher()
                    .map_err(|error| format!("{error:?}"))?;
                let parts = RuntimeParts {
                    producer,
                    selection: None,
                    policy: options.policy,
                    model_capacity: options.model_capacity,
                };
                services
                    .register_runtime(&mut builder, &Ports, parts, dispatcher, None)
                    .map_err(str::to_owned)?;
            }
        }
        Ok(Self {
            engine: builder.build().map_err(|error| error.to_string())?,
            sink: services.source.clone(),
            iec,
            children_read,
        })
    }

    pub(super) fn call(&self, name: &str, body: Value) -> ResultEnvelope {
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

    pub(super) fn ok(&self, name: &str, body: Value) -> Result<Value, String> {
        let reply = self.call(name, body);
        match reply.error {
            Some(error) => Err(format!("{name}: {}", error.reason_key)),
            None => Ok(reply.body),
        }
    }

    /// The error reason of a call that must fail.
    pub(super) fn reason(&self, name: &str, body: Value) -> Result<String, String> {
        let reply = self.call(name, body);
        reply
            .error
            .map(|error| error.reason_key)
            .ok_or_else(|| format!("{name} unexpectedly succeeded: {}", reply.body))
    }

    pub(super) fn open_view(&self) -> Result<Value, String> {
        Ok(self.ok("viz.view.open", json!({}))?["viewChannel"].clone())
    }

    /// Publishes `files` under one scope root and returns the lineage it belongs to.
    pub(super) fn publish(&self, publication: &Publication) -> Result<Id, String> {
        publication.publish(&self.sink)
    }
}

/// One scan result: lineage fingerprint, version identifiers and the files it holds.
pub(super) struct Publication<'a> {
    pub(super) fingerprint: &'a str,
    pub(super) lineage_candidate: u8,
    pub(super) scope: u8,
    pub(super) revision: u8,
    pub(super) files: &'a [(u8, Option<u64>)],
}

impl Publication<'_> {
    fn publish(&self, sink: &IngestSink) -> Result<Id, String> {
        let debug = |error: &dyn std::fmt::Debug| format!("{error:?}");
        let lineage = sink
            .bind_current_analysis(self.fingerprint, id(self.lineage_candidate))
            .map_err(|error| debug(&error))?;
        let root = ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/fixture"),
            },
            volume_id: vec![1],
            file_id: vec![2],
        };
        let mut scan = sink
            .open_forest_in_lineage(id(self.scope), vec![root], lineage.clone())
            .map_err(|error| debug(&error))?;
        scan.add_directory(vec![]).map_err(|error| debug(&error))?;
        for (name, bytes) in self.files {
            scan.add_file(Observation {
                path: vec![vec![*name]],
                size: bytes.map(|bytes| SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: bytes.into(),
                    },
                }),
            })
            .map_err(|error| debug(&error))?;
        }
        scan.seal(id(self.revision), ResultCompleteness::Complete)
            .map_err(|error| debug(&error))?;
        Ok(lineage)
    }
}

/// A render request body for the root folder of `revision`.
pub(super) fn render_body(view: &Value, seq: u32, format: &str, revision: u8) -> Value {
    let scope = json!({"resultVersion": id(revision), "itemNo": 0});
    json!({
        "viewChannel": view, "requestSeq": seq, "format": format, "resultRevision": id(revision),
        "aggregateScopeRef": scope, "navigationScopeRef": scope,
        "measure": "size", "sizeBasis": "logical",
        "classificationCriterion": "none", "colorCriterion": "none",
        "denominatorBasis": "scope", "viewport": {"width": 800, "height": 600, "scale": 1.0},
        "visibleWindow": {"left": 0, "top": 0, "right": 1_000_000, "bottom": 1_000_000},
        "minAreaPx2": 16, "depthLimit": 1,
    })
}

/// A producer that reports data for trend, comparison and attribution that no source has.
struct Overclaiming {
    inner: ProductionEvidence,
    children_read: Arc<AtomicUsize>,
}

impl EvidenceProducer for Overclaiming {
    fn supports_required_data_availability(&self) -> bool {
        true
    }

    fn required_data_facts(
        &self,
        source: &CurrentAnalysis,
        request: &VizRenderRequest,
    ) -> Result<Option<FactValues>, EngineError> {
        Ok(self
            .inner
            .required_data_facts(source, request)?
            .map(|facts| FactValues {
                trend_observations: Evidence::Known(2),
                comparison: Evidence::Known(()),
                attribution: Evidence::Known(Attribution {
                    has_negative_flow: false,
                }),
                ..facts
            }))
    }

    fn source_policy_facts(
        &self,
        source: &CurrentAnalysis,
        request: &VizRenderRequest,
    ) -> Result<Option<SourcePolicyFacts>, EngineError> {
        self.inner.source_policy_facts(source, request)
    }

    fn histogram_facts(
        &self,
        source: &CurrentAnalysis,
        request: &VizRenderRequest,
    ) -> Result<Option<HistogramFacts>, EngineError> {
        self.inner.histogram_facts(source, request)
    }

    fn child(
        &self,
        source: &CurrentAnalysis,
        item: &ResultItem,
        request: &VizRenderRequest,
    ) -> Result<ItemEvidence, EngineError> {
        self.children_read.fetch_add(1, Ordering::SeqCst);
        self.inner.child(source, item, request)
    }
}
