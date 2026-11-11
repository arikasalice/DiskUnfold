//! Persist recent scopes after the WebView confirms successful receipt delivery.
use std::{
    io,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::TryRecvError,
    },
    thread::{self, Thread},
};

use du_app_db::recent_scope::{RecentError, RecentWrite};
use du_core_contracts::{ENVELOPE_VERSION, ResultEnvelope, analysis_ipc::ScanStart, ports::Random};
use du_engine_api::desktop::{RecentScopeAdmission, RecentScopeAdmissions, RecentScopeDelivery};
use du_platform_os::{
    entropy::OsRandom,
    webview::{ResponseDelivery, ResponseDeliveryObserver},
};

use crate::{
    recent_scope_body::{BodyError, RecentScopeBody},
    shared_database::SharedDatabase,
};

/// A value-free warning about an accepted scan's recent-scope storage attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentScopeWriteWarning {
    InvalidEvidence,
    Busy,
    DiskFull,
    StorageUnavailable,
}

/// Owns the recent-scope consumer and stops its background work when dropped.
pub(crate) struct RecentScopeWorker {
    delivery: RecentScopeDelivery,
    stopped: Arc<AtomicBool>,
    thread: Thread,
}

impl RecentScopeWorker {
    /// Starts one consumer with the composition's storage cap and warning sink.
    pub(crate) fn start(
        admissions: RecentScopeAdmissions,
        database: SharedDatabase,
        cap: NonZeroUsize,
        warning: impl Fn(RecentScopeWriteWarning) + Send + 'static,
    ) -> io::Result<Self> {
        let delivery = admissions.delivery();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let worker = thread::Builder::new()
            .name("diskunfold-recent-scope".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    match admissions.try_next() {
                        Ok(candidate) => {
                            if let Err(error) = persist(&database, cap, &candidate, &stop)
                                && !stop.load(Ordering::Acquire)
                            {
                                warning(error);
                            }
                        }
                        Err(TryRecvError::Empty) => thread::park(),
                        Err(TryRecvError::Disconnected) => break,
                    }
                }
            })?;
        Ok(Self {
            delivery,
            stopped,
            thread: worker.thread().clone(),
        })
    }

    /// Classifies each reply on its IPC worker and wakes storage after delivery.
    pub(crate) fn observer(&self) -> ResponseDeliveryObserver {
        let delivery = self.delivery.clone();
        let thread = self.thread.clone();
        Arc::new(move |request, response| {
            let receipt = new_scan_receipt(request, response)?;
            let ticket = delivery.claim(&receipt.job_ref)?;
            let thread = thread.clone();
            Some(Box::new(move |outcome| {
                match outcome {
                    ResponseDelivery::Posted => ticket.posted(),
                    ResponseDelivery::NotPosted => drop(ticket),
                }
                thread.unpark();
            }))
        })
    }
}

impl Drop for RecentScopeWorker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

fn new_scan_receipt(request: &str, response: &str) -> Option<ScanStart> {
    if crate::ipc::gui_command_name(request).as_deref() != Some("scan.start") {
        return None;
    }
    let envelope: ResultEnvelope = serde_json::from_str(response).ok()?;
    if envelope.name != "scan.start"
        || envelope.error.is_some()
        || envelope.contract_version != ENVELOPE_VERSION
    {
        return None;
    }
    let receipt: ScanStart = serde_json::from_value(envelope.body).ok()?;
    (!receipt.existing).then_some(receipt)
}

fn persist(
    database: &SharedDatabase,
    cap: NonZeroUsize,
    candidate: &RecentScopeAdmission,
    stopped: &AtomicBool,
) -> Result<(), RecentScopeWriteWarning> {
    let now_ms = crate::settings::current_time_ms();
    let encoded = match RecentScopeBody::encode_admission(candidate, stopped) {
        Ok(encoded) => encoded,
        Err(BodyError::EvidenceUnavailable) => return Ok(()),
        Err(_) => return Err(RecentScopeWriteWarning::InvalidEvidence),
    };
    let item = RecentWrite {
        id: recent_id(now_ms)?,
        fingerprint: encoded.fingerprint,
        body: &encoded.body,
        include_count: encoded.include_count,
        now_ms,
    };
    database
        .put_recent_scope(cap, &item, stopped)
        .map_err(|error| match error {
            RecentError::Busy => RecentScopeWriteWarning::Busy,
            RecentError::DiskFull => RecentScopeWriteWarning::DiskFull,
            RecentError::Invalid => RecentScopeWriteWarning::InvalidEvidence,
            RecentError::Corrupt | RecentError::Storage => {
                RecentScopeWriteWarning::StorageUnavailable
            }
        })
}

fn recent_id(now_ms: i64) -> Result<[u8; 16], RecentScopeWriteWarning> {
    let mut bytes = [0; 16];
    OsRandom
        .fill(&mut bytes)
        .map_err(|_| RecentScopeWriteWarning::StorageUnavailable)?;
    bytes[..6].copy_from_slice(&now_ms.to_be_bytes()[2..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes)
}
