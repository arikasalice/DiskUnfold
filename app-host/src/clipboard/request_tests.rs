use super::*;
use du_core_contracts::{
    ENVELOPE_VERSION, Id, OsPath, PathBytes, PathEncoding, ResultEnvelope,
    analysis_ipc::{AnalysisJobStatus, ScanStart, ScopeReview},
    job::{HistoryAppend, JobHistory, JobSummary},
    ports::{ClipboardPort, PortError, PortErrorKind, Random},
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, FilterDecision, FilteredBatch, ObservedEntry,
        OpenedScopeRoot, ScopeRootPort,
    },
    volume::{
        Capability, CapabilityState, VolumeCapabilities, VolumeCapacity, VolumeEnumeration,
        VolumePort,
    },
};
use du_engine_api::api::{Engine, EngineBuilder};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Clone, Default)]
struct Clipboard(Arc<Mutex<Vec<String>>>);

impl ClipboardPort for Clipboard {
    fn can_write_text(&self) -> bool {
        true
    }

    fn write_text(&self, text: &str) -> Result<(), PortError> {
        self.0
            .lock()
            .map_err(|_| unavailable())?
            .push(text.to_owned());
        Ok(())
    }
}

struct Roots;

impl ScopeRootPort for Roots {
    fn open_root(&self, absolute_path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        Ok(OpenedScopeRoot {
            canonical_path: OsPath {
                encoding: PathEncoding::WindowsUtf16le,
                raw: PathBytes::from_bytes(absolute_path),
                display: "synthetic-root".into(),
                lossy: false,
            },
            volume_id: b"fixture-volume".to_vec(),
            file_id: absolute_path.to_vec(),
            directory: Box::new(Directory),
        })
    }
}

struct Directory;

impl DirectoryObservations for Directory {
    fn next_batch(
        &mut self,
    ) -> Result<Vec<du_core_contracts::scan_observation::ObservedEntry>, PortError> {
        Ok(vec![])
    }

    fn next_filtered(
        &mut self,
        _: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        Ok(FilteredBatch::End)
    }
}

impl AnchoredDirectory for Directory {
    fn open_child(&self, _: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        Err(unavailable())
    }
}

struct Volumes;

impl VolumePort for Volumes {
    fn enumerate(&self) -> VolumeEnumeration {
        let unsupported = Capability {
            state: CapabilityState::Unsupported,
            reason_key: Some("os.unsupported"),
        };
        VolumeEnumeration {
            mounts: Ok(vec![]),
            capabilities: VolumeCapabilities {
                enumerate: unsupported.clone(),
                identity: unsupported.clone(),
                capacity: unsupported.clone(),
                path_access: unsupported.clone(),
                path_compare_rule: unsupported.clone(),
                mount_boundary: unsupported.clone(),
                alias: unsupported,
            },
        }
    }

    fn path_access(&self, _: &[u8]) -> Result<(), PortError> {
        Ok(())
    }

    fn capacity(&self, _: &[u8]) -> Result<VolumeCapacity, PortError> {
        Err(unavailable())
    }
}

struct History;

impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}

struct TestRandom(AtomicUsize);

impl Default for TestRandom {
    fn default() -> Self {
        Self(AtomicUsize::new(1))
    }
}

impl Random for TestRandom {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        let seed = self.0.fetch_add(1, Ordering::Relaxed) as u8;
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = seed.wrapping_add(index as u8);
        }
        Ok(())
    }
}

fn unavailable() -> PortError {
    PortError {
        kind: PortErrorKind::Unsupported,
        reason_key: "os.unsupported".into(),
        os_code: None,
    }
}

fn id(byte: u8) -> Id {
    Id::from_v7_bytes([byte; 16])
}

fn ipc(
    engine: &Engine,
    name: &str,
    body: Value,
) -> Result<ResultEnvelope, Box<dyn std::error::Error>> {
    let correlation_id = id(42);
    let message = json!({
        "command": "du_call",
        "args": {"envelope": {
            "name": name,
            "contractVersion": ENVELOPE_VERSION,
            "correlationId": correlation_id,
            "body": body
        }}
    });
    let response = crate::ipc::dispatch(engine, &message.to_string()).ok_or("no IPC response")?;
    let envelope: ResultEnvelope = serde_json::from_str(&response)?;
    assert_eq!(envelope.correlation_id, correlation_id);
    Ok(envelope)
}

fn engine_and_clipboard(
    path_text_calls: Arc<AtomicUsize>,
) -> Result<(Engine, Clipboard), Box<dyn std::error::Error>> {
    let mut builder = EngineBuilder::default();
    let desktop = du_engine_api::desktop::register_services(
        &mut builder,
        Arc::new(Roots),
        Arc::new(Volumes),
        Arc::new(History),
        &TestRandom::default(),
        Arc::new(du_engine_api::desktop::SharedTableLimit::default),
    )?;
    let clipboard = Clipboard::default();
    let path_text_calls_for_copy = path_text_calls;
    builder.register(CopyPath::new(
        desktop.path_copy,
        clipboard.clone(),
        move |_: &du_core_contracts::result::ResultNativePath| {
            let call = path_text_calls_for_copy.fetch_add(1, Ordering::Relaxed);
            Some(if call == 1 {
                "synthetic\npath".into()
            } else {
                "synthetic-path".into()
            })
        },
        "\r\n",
    ));
    Ok((builder.build()?, clipboard))
}

fn pinned_version(engine: &Engine) -> Result<Id, Box<dyn std::error::Error>> {
    let includes = ["C:\\F05Fixture", "C:\\F05Fixture2"]
        .into_iter()
        .map(|display| {
            let raw: Vec<_> = display.encode_utf16().flat_map(u16::to_le_bytes).collect();
            json!({
                "encoding": "windows_utf16le",
                "raw": PathBytes::from_bytes(&raw),
                "display": display,
                "lossy": false
            })
        })
        .collect::<Vec<_>>();
    let draft = json!({
        "includes": includes,
        "exclusions": [],
        "boundary": "current_volume",
        "includedMounts": [],
        "source": "gui"
    });
    let review = ipc(
        engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    assert!(review.error.is_none(), "{:?}", review.error);
    let review: ScopeReview = serde_json::from_value(review.body)?;
    let started = ipc(
        engine,
        "scan.start",
        json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
    )?;
    assert!(started.error.is_none(), "{:?}", started.error);
    let started: ScanStart = serde_json::from_value(started.body)?;
    for _ in 0..200 {
        let status = ipc(engine, "job.status", json!({"jobRef": started.job_ref}))?;
        assert!(status.error.is_none(), "{:?}", status.error);
        let status: AnalysisJobStatus = serde_json::from_value(status.body)?;
        if let Some(version) = status.result_version {
            return Ok(version);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err("fixture scan did not publish a result version".into())
}

#[test]
fn registered_copy_path_rejects_invalid_item_refs_before_clipboard_access()
-> Result<(), Box<dyn std::error::Error>> {
    let path_text_calls = Arc::new(AtomicUsize::new(0));
    let (engine, clipboard) = engine_and_clipboard(Arc::clone(&path_text_calls))?;
    let version = pinned_version(&engine)?;

    for body in [
        json!({
            "resultVersion": version,
            "items": [{"resultVersion": version, "itemNo": 0, "futureItemField": true}]
        }),
        json!({"resultVersion": version, "items": [[version, 0]]}),
    ] {
        let rejected = ipc(&engine, "browse.copy_path", body)?;
        assert_eq!(
            rejected
                .error
                .ok_or("missing invalid-body error")?
                .reason_key,
            "command.body_invalid"
        );
    }
    assert_eq!(path_text_calls.load(Ordering::Relaxed), 0);
    assert!(
        clipboard
            .0
            .lock()
            .map_err(|_| "clipboard mutex poisoned")?
            .is_empty()
    );

    let copied = ipc(
        &engine,
        "browse.copy_path",
        json!({
            "resultVersion": version,
            "items": [{"resultVersion": version, "itemNo": 0}]
        }),
    )?;
    assert!(copied.error.is_none(), "{:?}", copied.error);
    assert_eq!(path_text_calls.load(Ordering::Relaxed), 1);
    assert_eq!(copied.body, json!({"outcome":"copied","count":1}));
    assert_eq!(
        *clipboard.0.lock().map_err(|_| "clipboard mutex poisoned")?,
        ["synthetic-path"]
    );

    path_text_calls.store(0, Ordering::Relaxed);
    let refused = ipc(
        &engine,
        "browse.copy_path",
        json!({
            "resultVersion": version,
            "items": [
                {"resultVersion": version, "itemNo": 0},
                {"resultVersion": version, "itemNo": 1}
            ]
        }),
    )?;
    assert!(refused.error.is_none(), "{:?}", refused.error);
    assert_eq!(
        refused.body,
        json!({"outcome":"refused","reasonKey":"path.multiline"})
    );
    assert_eq!(path_text_calls.load(Ordering::Relaxed), 2);
    assert_eq!(
        *clipboard.0.lock().map_err(|_| "clipboard mutex poisoned")?,
        ["synthetic-path"]
    );
    Ok(())
}
