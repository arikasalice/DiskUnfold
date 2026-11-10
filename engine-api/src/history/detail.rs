//! Main-window history detail and workflow composition.

use du_core_contracts::{
    Id,
    history_ipc::{
        HistoryDetail, HistoryDetailCompleteness, HistoryDetailRequest, HistoryEffects,
        HistoryFollowUp, HistoryListSummary, HistoryRelation, HistoryWorkflow,
    },
};

use crate::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

/// Readable parts of one history record captured in the same storage snapshot.
pub struct StoredDetail {
    pub summary: Option<HistoryListSummary>,
    pub executed_plan: Option<Id>,
    pub requires_plan_read: bool,
    pub follow_ups: Option<Vec<HistoryFollowUp>>,
    pub missing_reasons: Vec<String>,
}

/// A failure of the detail reader as a whole.
#[derive(Debug, Clone, Copy)]
pub struct StorageUnavailable;

/// The detail lookup used by the GUI, independently of the list cursor.
pub trait Read {
    /// A missing record is distinct from an inaccessible store or a partial record.
    fn get(&self, job: &Id) -> Result<Option<StoredDetail>, StorageUnavailable>;
}

/// Registers the read-only history detail command on the GUI channel.
pub fn register<R: Read + Send + Sync + 'static>(builder: &mut EngineBuilder, reader: R) {
    builder.register(Detail(reader));
}

struct Detail<R>(R);

impl<R: Read + Send + Sync + 'static> UseCase for Detail<R> {
    const NAME: &'static str = "history.get";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = HistoryDetailRequest;
    type Response = HistoryDetail;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let stored = self
            .0
            .get(&request.job_ref)
            .map_err(|_| EngineError::reason("history.storage_unavailable"))?
            .ok_or_else(|| EngineError::reason("history.removed"))?;
        Ok(compose(stored))
    }
}

fn compose(mut stored: StoredDetail) -> HistoryDetail {
    if stored.requires_plan_read {
        stored.missing_reasons.push("plan_unavailable".into());
    }
    // A recorded effect check is evidence that there may be a measurement to
    // read. Until its provider is wired, keep that value explicitly unknown.
    if stored
        .follow_ups
        .iter()
        .flatten()
        .any(|row| row.relation == Some(HistoryRelation::EffectCheck))
        && !stored
            .missing_reasons
            .iter()
            .any(|reason| reason == "read_failed")
    {
        stored.missing_reasons.push("read_failed".into());
    }
    HistoryDetail {
        summary: stored.summary,
        workflow: HistoryWorkflow {
            plan_versions: (!stored.requires_plan_read).then(Vec::new),
            executed_plan: stored.executed_plan,
            approval_check: None,
        },
        follow_ups: stored.follow_ups,
        effects: HistoryEffects::default(),
        completeness: if stored.missing_reasons.is_empty() {
            HistoryDetailCompleteness::Complete
        } else {
            HistoryDetailCompleteness::Partial
        },
        missing_reasons: stored.missing_reasons,
    }
}
