//! The embedded bundle through the real job runtime and the shared result store.

use std::sync::{Arc, Mutex, atomic::AtomicUsize, atomic::Ordering};
use std::time::Duration;

use du_analysis_index::IngestSink;
use du_core_contracts::{
    data_source::DataSource,
    job::JobState,
    result::ResultVersionRead,
    sample_ipc::{SAMPLE_BUNDLE_INVALID, SAMPLE_FORMAT_UNSUPPORTED, SAMPLE_MISSING, SampleSession},
};
use du_job_runtime::{FlowLimits, JobRuntime};
use du_scan_engine::sample_bundle::BundleFailure;
use serde_json::json;

use super::{
    Sink,
    engine::call,
    real::{Clock, Downstream, History, Ids, wait_for},
};
use crate::{
    api::{Channel, EngineBuilder},
    sample::{EmbeddedSampleLoader, SampleEvents, embedded::unavailable_reason, register},
    workers::Workers,
};

#[test]
fn failures_map_to_their_unavailable_reasons() {
    assert_eq!(unavailable_reason(BundleFailure::Missing), SAMPLE_MISSING);
    assert_eq!(
        unavailable_reason(BundleFailure::UnsupportedFormat),
        SAMPLE_FORMAT_UNSUPPORTED
    );
    assert_eq!(
        unavailable_reason(BundleFailure::Corrupt),
        SAMPLE_BUNDLE_INVALID
    );
}

#[test]
fn the_embedded_bundle_loads_sample_versions_that_close_releases() -> Result<(), String> {
    let history = Arc::new(History::default());
    let downstream = Arc::new(Downstream(Mutex::default()));
    let events = Arc::new(SampleEvents::new(downstream.clone()));
    let ids = Arc::new(Ids(AtomicUsize::new(0)));
    let runtime = Arc::new(
        JobRuntime::new_with_limits(
            ids.clone(),
            Arc::new(Clock),
            history.clone(),
            events.clone(),
            FlowLimits {
                metadata_io: 16,
                queued_items: 64,
                item_bytes: 256 * 1024,
            },
        )
        .map_err(|e| format!("{e:?}"))?,
    );
    let store = IngestSink::with_version_publisher(runtime.result_version_publisher());
    let workers = Arc::new(Workers::start(runtime).map_err(|e| e.to_string())?);
    let loader = Arc::new(EmbeddedSampleLoader::new(store.clone(), ids));
    let mut builder = EngineBuilder::default();
    let sessions = register(&mut builder, workers, loader, Arc::new(Sink::default()))
        .map_err(|e| format!("{e:?}"))?;
    events.bind(sessions)?;
    let engine = builder.build().map_err(|e| format!("{e:?}"))?;

    let opened = call(&engine, "sample.open", Channel::Gui, json!({}));
    assert!(opened.error.is_none(), "{:?}", opened.error);
    let opened: SampleSession = serde_json::from_value(opened.body).map_err(|e| e.to_string())?;
    let reference = json!({"sessionRef": opened.session_ref.as_str()});
    let read = || {
        let status = call(&engine, "sample.status", Channel::Gui, reference.clone());
        serde_json::from_value::<SampleSession>(status.body).map_err(|e| e.to_string())
    };
    wait_for(|| {
        read().is_ok_and(|view| {
            view.state == du_core_contracts::sample_ipc::SampleSessionState::Active
        })
    })?;
    let active = read()?;
    assert_eq!(active.observations.len(), 2);
    assert!(active.job_ref.is_none() && active.bundle_version.is_some());
    for observation in &active.observations {
        let version = store
            .pin_current(&observation.result_version)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(version.data_source(), DataSource::Sample);
        assert_eq!(version.observed_at(), observation.observed_at.as_ref());
    }
    // A sample version is never the implicit current result.
    assert!(store.pin().map_err(|e| format!("{e:?}"))?.is_none());
    // The terminal event also reaches the downstream receiver; no history is written.
    wait_for(|| {
        downstream
            .0
            .lock()
            .is_ok_and(|list| list.contains(&JobState::Done))
    })?;
    assert_eq!(history.0.load(Ordering::Relaxed), 0);

    let closed = call(&engine, "sample.close", Channel::Gui, reference.clone());
    assert_eq!(closed.body["releasedVersions"], 2);
    for observation in &active.observations {
        assert!(store.pin_current(&observation.result_version).is_err());
    }
    let again = call(&engine, "sample.close", Channel::Gui, reference);
    assert_eq!(again.body, closed.body);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(history.0.load(Ordering::Relaxed), 0);
    Ok(())
}
