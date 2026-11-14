//! F36 virtual-group use cases: rule storage, pinned classification and reads.
//!
//! Groups classify observed items only. No use case here reaches the change
//! planner or executor, and none edits observed values.
mod channels;
mod classify;
mod codec;
mod read;
mod rules;
mod summary;
mod tables;
#[cfg(test)]
mod tests;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use du_analysis_index::{
    IngestSink,
    group::{Classification, ClassificationStore},
    query::Registry,
};
use du_core_contracts::{
    Id,
    group_rules::GroupRulesStore,
    job::{HistoryMode, JobControl, JobDescriptor, ProgressUnit, ResourceClass},
};
use du_job_runtime::{JobClock, JobIdentity, JobRuntime, RuntimeError};

use crate::{api::EngineBuilder, error::EngineError, workers::Workers};

pub(crate) const JOB_KIND: &str = "group.classify";
/// Retained metadata entries; the classification store itself keeps at most eight.
const META_LIMIT: usize = 64;

/// Composition inputs for the group use cases, created with the desktop services.
pub struct GroupAssembly {
    pub(crate) store: IngestSink,
    pub(crate) runtime: Arc<JobRuntime>,
    pub(crate) workers: Arc<Workers>,
    pub(crate) ids: Arc<dyn JobIdentity>,
    pub(crate) clock: Arc<dyn JobClock>,
}

impl GroupAssembly {
    /// Registers the eight GUI-only `group.*` use cases and the `group.classify`
    /// job kind. The GUI composition supplies the rule store; CLI does not.
    pub fn register(
        self,
        builder: &mut EngineBuilder,
        rules: Arc<dyn GroupRulesStore>,
    ) -> Result<(), RuntimeError> {
        self.runtime.register(descriptor())?;
        let services = Arc::new(Services {
            assembly: self,
            rules,
            registry: Registry::baseline(),
            registered: Arc::new(Registered::default()),
            channels: channels::Channels::default(),
        });
        builder.register(rules::Get(services.clone()));
        builder.register(rules::Save(services.clone()));
        builder.register(classify::Classify(services.clone()));
        builder.register(summary::Summary(services.clone()));
        builder.register(summary::Cancel(services.clone()));
        builder.register(read::Members(services.clone()));
        builder.register(read::Explain(services.clone()));
        builder.register(read::Totals(services));
        Ok(())
    }
}

/// Builds the group assembly from the desktop composition's shared services.
pub(crate) fn compose(
    store: &IngestSink,
    runtime: &Arc<JobRuntime>,
    workers: &Arc<Workers>,
    ids: Arc<dyn JobIdentity>,
) -> GroupAssembly {
    GroupAssembly {
        store: store.clone(),
        runtime: runtime.clone(),
        workers: workers.clone(),
        ids,
        clock: Arc::new(UtcClock),
    }
}

/// Wall-clock UTC time for rule save timestamps.
struct UtcClock;

impl JobClock for UtcClock {
    fn now_ms(&self) -> i64 {
        std::time::UNIX_EPOCH
            .elapsed()
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
            .unwrap_or(0)
    }
}

fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: JOB_KIND.into(),
        version: 1,
        display_name_key: JOB_KIND.into(),
        controls: vec![JobControl::Cancel],
        safe_points: Vec::new(),
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::Compute],
        history: HistoryMode::None,
    }
}

pub(crate) struct Services {
    assembly: GroupAssembly,
    rules: Arc<dyn GroupRulesStore>,
    registry: Registry,
    registered: Arc<Registered>,
    channels: channels::Channels,
}

/// Rule identity and source data captured when a classification is admitted.
#[derive(Clone, Debug)]
pub(crate) struct Meta {
    pub(crate) group_ids: Vec<Id>,
    pub(crate) names: Vec<String>,
    /// Zero for a draft.
    pub(crate) ruleset_version: u32,
    pub(crate) fingerprint: String,
    pub(crate) key: String,
}

/// Registered classifications and their metadata. Holds no runtime reference,
/// so job publication callbacks can own it without a reference cycle.
#[derive(Default)]
pub(crate) struct Registered {
    store: ClassificationStore,
    meta: Mutex<VecDeque<(Id, Arc<Meta>)>>,
    /// Job references and the classification each job registers when it completes.
    pending: Mutex<VecDeque<(Id, Id)>>,
}

impl Registered {
    fn track(&self, job: Id, reference: Id) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push_back((job, reference));
            while pending.len() > META_LIMIT {
                pending.pop_front();
            }
        }
    }

    fn tracked(&self, job: &Id) -> Option<Id> {
        let pending = self.pending.lock().ok()?;
        pending
            .iter()
            .find(|(known, _)| known == job)
            .map(|(_, reference)| reference.clone())
    }

    fn register(
        &self,
        id: Id,
        classification: Classification,
        meta: Arc<Meta>,
    ) -> Result<(), EngineError> {
        self.store
            .register(id.clone(), Arc::new(classification))
            .map_err(|_| EngineError::reason("support.internal_error"))?;
        let mut entries = self.meta.lock().map_err(|_| internal())?;
        entries.retain(|(known, _)| known != &id);
        entries.push_back((id, meta));
        while entries.len() > META_LIMIT {
            entries.pop_front();
        }
        Ok(())
    }

    /// Returns a registered classification and its metadata, or `group.result_expired`.
    fn get(
        &self,
        id: &Id,
        store: &IngestSink,
    ) -> Result<(Arc<Classification>, Arc<Meta>), EngineError> {
        // A classification whose source is no longer the current pinned result is released.
        self.store.retain(|classification| {
            use du_core_contracts::result::ResultVersionRead;
            store
                .pin_current(classification.version().version_id())
                .is_ok()
        });
        let expired = || EngineError::reason("group.result_expired");
        let classification = self.store.get(id).ok_or_else(expired)?;
        let meta = self
            .meta
            .lock()
            .map_err(|_| internal())?
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, meta)| meta.clone())
            .ok_or_else(expired)?;
        Ok((classification, meta))
    }

    /// Finds a registered classification with the same duplicate key.
    fn find(&self, key: &str, store: &IngestSink) -> Option<Id> {
        let candidates: Vec<Id> = self
            .meta
            .lock()
            .ok()?
            .iter()
            .rev()
            .filter(|(_, meta)| meta.key == key)
            .map(|(id, _)| id.clone())
            .collect();
        candidates
            .into_iter()
            .find(|id| self.get(id, store).is_ok())
    }
}

pub(crate) fn internal() -> EngineError {
    EngineError::reason("support.internal_error")
}

/// Lower-case hexadecimal rendering of a fingerprint.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}
