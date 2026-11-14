//! Every command in the composed desktop engine requires an object request body.
use std::{error::Error, num::NonZeroUsize, sync::Arc, time::Duration};

use du_core_contracts::{
    ENVELOPE_VERSION, Id,
    history_ipc::{HistoryItemOutcome, HistoryListRow},
    job::{HistoryAppend, JobHistory, JobSummary},
    ports::{PortError, PortErrorKind, Random},
    result::ResultItem,
    scan_observation::{OpenedScopeRoot, ScopeRootPort},
    visualization::request::VizRenderRequest,
    volume::{
        Capability, CapabilityState, VolumeCapabilities, VolumeCapacity, VolumeEnumeration,
        VolumePort,
    },
};
use serde_json::{Value, json};

use super::call;
use crate::{
    api::{Channel, Engine, EngineBuilder, OutboundFilter},
    current_analysis::CurrentAnalysis,
    desktop::{SharedTableLimit, VisualizationPreview},
    error::EngineError,
    history::{
        self, Filter, Position, ReadError, ReadPage, Revision,
        detail::{Read as DetailRead, StorageUnavailable, StoredDetail},
        items::{Cursor, Page, Read as ItemsRead, ReadError as ItemsError},
    },
    viz_catalog,
    viz_runtime::{
        EvidenceProducer, ItemEvidence, ViewPolicy,
        selection::{SelectionError, WindowScanSelection},
    },
};

struct Fixture;

impl Random for Fixture {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(7);
        Ok(())
    }
}

impl JobHistory for Fixture {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}

impl ScopeRootPort for Fixture {
    fn open_root(&self, _: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        Err(unsupported())
    }
}

impl VolumePort for Fixture {
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

/// History readers that always report an unavailable store; no command executes.
struct Unavailable;

impl history::HistoryRead for Unavailable {
    type Summary = HistoryListRow;

    fn page(
        &self,
        _: &Filter,
        _: Option<Revision>,
        _: Option<Position>,
        _: usize,
    ) -> Result<ReadPage<HistoryListRow>, ReadError> {
        Err(ReadError::Unavailable)
    }
}

impl DetailRead for Unavailable {
    fn get(&self, _: &Id) -> Result<Option<StoredDetail>, StorageUnavailable> {
        Err(StorageUnavailable)
    }
}

impl ItemsRead for Unavailable {
    fn page(
        &self,
        _: &Id,
        _: Option<HistoryItemOutcome>,
        _: Option<Cursor>,
        _: usize,
    ) -> Result<Page, ItemsError> {
        Err(ItemsError::StorageUnavailable)
    }
}

impl WindowScanSelection for Fixture {
    fn selected_result_revision(&self, _: &Id) -> Result<Option<Id>, SelectionError> {
        Ok(None)
    }
}

impl EvidenceProducer for Fixture {
    fn supports_required_data_availability(&self) -> bool {
        true
    }

    fn child(
        &self,
        _: &CurrentAnalysis,
        _: &ResultItem,
        _: &VizRenderRequest,
    ) -> Result<ItemEvidence, EngineError> {
        Err(EngineError::new("ref.unavailable", vec![]))
    }
}

impl OutboundFilter for Fixture {
    fn filter(
        &self,
        _: &str,
        _: &crate::api::CallerCtx,
        response: Value,
    ) -> Result<Value, EngineError> {
        Ok(response)
    }
}

/// Composes the desktop analysis flow, the history reads and the visualization preview.
fn composed_engine() -> Result<Engine, Box<dyn Error>> {
    let mut builder = EngineBuilder::default();
    let random = &Fixture;
    let desktop = crate::desktop::register_services(
        &mut builder,
        Arc::new(Fixture),
        Arc::new(Fixture),
        Arc::new(Fixture),
        random,
        Arc::new(SharedTableLimit::default),
    )?;
    desktop
        .visualization
        .register_preview_with_catalog_and_availability(
            &mut builder,
            random,
            VisualizationPreview {
                producer: Arc::new(Fixture),
                selection: Arc::new(Fixture),
                policy: ViewPolicy {
                    revision: 1,
                    max_scale: 4.0,
                    max_shapes: 100,
                    max_channels: NonZeroUsize::MIN,
                    max_work: NonZeroUsize::MIN,
                    synchronous_wait: Duration::from_millis(100),
                },
                model_capacity: NonZeroUsize::MIN,
            },
            viz_catalog::Policy {
                measures: vec!["size".into()],
                color_criteria: vec!["type".into()],
                denominator_criteria: vec!["scope".into()],
                interpretation_table_revision: 1,
                required_data_table_revision: 1,
                legend_rule_revision: 1,
            },
        )?;
    history::detail::register(&mut builder, Unavailable);
    let report = |error: PortError| format!("{error:?}");
    history::items::register(&mut builder, Unavailable, random).map_err(report)?;
    history::register(&mut builder, Unavailable, random).map_err(report)?;
    builder.with_outbound_filter(Fixture);
    Ok(builder.build()?)
}

fn reason(engine: &Engine, name: &str, channel: Channel, body: Value) -> Result<String, String> {
    let reply = call(engine, name, channel, ENVELOPE_VERSION, body)?;
    Ok(reply
        .error
        .map_or_else(String::new, |error| error.reason_key))
}

#[test]
fn every_composed_command_rejects_non_object_bodies_on_each_allowed_channel()
-> Result<(), Box<dyn Error>> {
    let engine = composed_engine()?;
    let commands = engine.registered_commands();
    // Guard against the composition silently dropping a registration group.
    for expected in [
        "browse.open",
        "history.get",
        "history.items",
        "history.list",
        "job.cancel",
        "scan.start",
        "scope.review",
        "space.total",
        "target.list",
        "viz.availability",
        "viz.catalog",
        "viz.render",
        "viz.rows",
    ] {
        assert!(
            commands.iter().any(|(name, _)| *name == expected),
            "{expected}"
        );
    }
    for (name, channels) in commands {
        assert!(!channels.is_empty(), "{name}");
        for &channel in channels {
            for body in [
                json!([]),
                json!([1]),
                json!("text"),
                json!(7),
                json!(true),
                json!(null),
            ] {
                assert_eq!(
                    reason(&engine, name, channel, body.clone())?,
                    "command.body_invalid",
                    "{name} {channel:?} {body}"
                );
            }
        }
    }
    // An object body still reaches the command.
    assert_eq!(reason(&engine, "viz.catalog", Channel::Gui, json!({}))?, "");
    Ok(())
}
