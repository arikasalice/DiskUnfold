//! Exercises the F02 cancellation checkpoint through desktop IPC and NTFS.

use super::*;
use du_core_contracts::{
    SizeAmount, SizeBasis, Timestamp,
    analysis_ipc::{AnalysisJobControl, AnalysisJobStatus, JobControlOutcome, SpaceTotal},
    ports::{PortError, PortErrorKind},
    result::ResultCompleteness,
    scan_observation::{
        AnchoredDirectory, DirectoryObservations, ObservedEntry, OpenedScopeRoot, ScopeRootPort,
        TimedObservedEntry, VolumeSpaceReader,
    },
};
use du_platform_os::windows_observation::WindowsScopeRoots;
use std::{
    fs,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
};

struct GatedRoots {
    waiting: mpsc::Sender<()>,
    resume: Arc<Mutex<mpsc::Receiver<()>>>,
    batches: Arc<AtomicUsize>,
}

impl ScopeRootPort for GatedRoots {
    fn open_root(&self, absolute_path: &[u8]) -> Result<OpenedScopeRoot, PortError> {
        let mut root = WindowsScopeRoots.open_root(absolute_path)?;
        root.directory = Box::new(GatedDirectory {
            inner: root.directory,
            waiting: self.waiting.clone(),
            resume: self.resume.clone(),
            batches: self.batches.clone(),
        });
        Ok(root)
    }
}

struct GatedDirectory {
    inner: Box<dyn AnchoredDirectory>,
    waiting: mpsc::Sender<()>,
    resume: Arc<Mutex<mpsc::Receiver<()>>>,
    batches: Arc<AtomicUsize>,
}

impl DirectoryObservations for GatedDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        self.next_batch_timed(&|| None)
            .map(|entries| entries.into_iter().map(|entry| entry.entry).collect())
    }

    fn next_batch_timed(
        &mut self,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<Vec<TimedObservedEntry>, PortError> {
        let batch_number = self.batches.fetch_add(1, Ordering::SeqCst) + 1;
        let entries = self.inner.next_batch_timed(clock)?;
        if batch_number == 2 {
            // Keep real NTFS observations in the in-flight port call until IPC accepts cancellation.
            self.waiting.send(()).map_err(|_| gate_failed())?;
            self.resume
                .lock()
                .map_err(|_| gate_failed())?
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| gate_failed())?;
        }
        Ok(entries)
    }
}

impl AnchoredDirectory for GatedDirectory {
    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        self.inner.open_child(entry)
    }

    fn volume_space_reader(&self) -> Result<Box<dyn VolumeSpaceReader>, PortError> {
        self.inner.volume_space_reader()
    }

    fn observed_modified_at_timed(
        &self,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> (Option<Timestamp>, Option<Timestamp>) {
        self.inner.observed_modified_at_timed(clock)
    }

    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        self.inner.ancestor_file_ids()
    }
}

fn gate_failed() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "scan.unclassified".into(),
        os_code: None,
    }
}

#[test]
fn cancellation_drains_the_inflight_native_batch_without_a_new_read()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    for file in 0..300 {
        fs::write(fixture.path().join(format!("file-{file:03}")), [1_u8])?;
    }
    let profile = tempfile::tempdir()?;
    let database = crate::startup_database::open(profile.path())?;
    let (waiting, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let batches = Arc::new(AtomicUsize::new(0));
    let mut builder = EngineBuilder::default();
    let random = &du_platform_os::entropy::OsRandom;
    let events = du_engine_api::desktop::register(
        &mut builder,
        Arc::new(GatedRoots {
            waiting,
            resume: Arc::new(Mutex::new(resume)),
            batches: batches.clone(),
        }),
        Arc::new(du_platform_os::volume::WindowsVolumes),
        Arc::new(database.history(|| 400)),
        random,
    )?;
    du_engine_api::history::register(
        &mut builder,
        HistoryReader::open(profile.path(), database)?,
        random,
    )
    .map_err(|error| format!("{error:?}"))?;
    let engine = builder.build()?;
    let scan = start_scan(&engine, fixture.path())?;

    reached.recv_timeout(Duration::from_secs(10))?;
    let accepted = cancel(&engine, &scan.job_ref)?;
    assert_eq!(accepted.outcome, JobControlOutcome::Accepted);
    assert_eq!(accepted.state, JobState::CancelRequested);
    let repeated = cancel(&engine, &scan.job_ref)?;
    assert_eq!(repeated.outcome, JobControlOutcome::NoEffect);
    let requested: AnalysisJobStatus = read(&engine, "job.get", json!({"jobRef": scan.job_ref}))?;
    assert_eq!(requested.state, JobState::CancelRequested);
    assert!(requested.result_version.is_none());
    release.send(())?;

    let events = terminal_batch(&events, &scan.job_ref)?;
    let finished: Vec<_> = events
        .iter()
        .filter(|event| {
            event.topic == "job.finished" && event.job_ref.as_ref() == Some(&scan.job_ref)
        })
        .collect();
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].body["state"], "cancelled");
    let version = finished[0]
        .result_version
        .as_ref()
        .ok_or("cancelled result version missing")?;
    verify_cancelled_result(&engine, &scan.job_ref, version)?;
    assert_eq!(batches.load(Ordering::SeqCst), 2);
    assert_eq!(
        cancel(&engine, &scan.job_ref)?.outcome,
        JobControlOutcome::AlreadyTerminal,
    );
    Ok(())
}

fn start_scan(engine: &Engine, path: &Path) -> Result<ScanStart, Box<dyn std::error::Error>> {
    let raw: Vec<_> = path
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let draft = json!({
        "includes": [{"encoding": "windows_utf16le", "raw": PathBytes::from_bytes(&raw),
            "display": path, "lossy": false}],
        "exclusions": [], "boundary": "current_volume", "includedMounts": [], "source": "gui"
    });
    let review: ScopeReview = read(
        engine,
        "scope.review",
        json!({"draft": draft, "requestSeq": 1}),
    )?;
    read(
        engine,
        "scan.start",
        json!({"draft": draft, "reviewFingerprint": review.review_fingerprint}),
    )
}

fn cancel(engine: &Engine, job: &Id) -> Result<AnalysisJobControl, Box<dyn std::error::Error>> {
    read(engine, "job.cancel", json!({"jobRef": job}))
}

fn verify_cancelled_result(
    engine: &Engine,
    job: &Id,
    version: &Id,
) -> Result<(), Box<dyn std::error::Error>> {
    let status: AnalysisJobStatus = read(engine, "job.get", json!({"jobRef": job}))?;
    assert_eq!(status.state, JobState::Cancelled);
    assert_eq!(status.result_version.as_ref(), Some(version));
    let totals: SpaceTotal = read(
        engine,
        "space.total",
        json!({"resultVersion": version, "requestSeq": 2, "target": {"kind": "scope_root"}}),
    )?;
    assert_eq!(totals.completeness, ResultCompleteness::Cancelled);
    let logical = totals
        .sizes
        .iter()
        .find(|entry| entry.size.basis == SizeBasis::Logical)
        .ok_or("missing logical total")?;
    assert_eq!(
        logical.size.amount,
        SizeAmount::LowerBound {
            bytes: 300_u64.into()
        }
    );
    let history: HistoryListResponse = read(engine, "history.list", json!({}))?;
    assert_eq!(history.rows.len(), 1);
    assert_eq!(history.rows[0].job_ref.as_ref(), Some(job));
    let summary = history.rows[0]
        .summary
        .as_ref()
        .ok_or("missing history summary")?;
    assert_eq!(summary.state, HistoryEndState::Cancelled);
    Ok(())
}

fn read<T: serde::de::DeserializeOwned>(
    engine: &Engine,
    name: &str,
    body: Value,
) -> Result<T, Box<dyn std::error::Error>> {
    let response = ipc(engine, name, body)?;
    if let Some(error) = response.error {
        return Err(format!("{name}: {error:?}").into());
    }
    Ok(serde_json::from_value(response.body)?)
}
