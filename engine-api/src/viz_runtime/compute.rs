use super::*;
use crate::viz_pages::render;
use du_core_contracts::job::{
    Checkpoint, HistoryMode, JobBody, JobContext, JobControl, JobCounts, JobDescriptor, JobOutcome,
    ProgressUnit, ResourceClass,
};
pub(super) fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind_id: "viz.compute".into(),
        version: 1,
        display_name_key: "job.viz.compute".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["path_range_end".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![ResourceClass::Compute],
        history: HistoryMode::None,
    }
}
pub(super) struct Compute {
    pub session: Arc<Session>,
    pub producer: Arc<dyn EvidenceProducer>,
    pub admitted: Arc<Admitted>,
    pub work: Arc<Work>,
    pub candidate: Arc<CandidateSlot>,
}
impl JobBody for Compute {
    fn kind_id(&self) -> &str {
        "viz.compute"
    }
    fn run(&self, context: Arc<dyn JobContext>) -> JobOutcome {
        let mut cancelled = || context.checkpoint() != Checkpoint::Continue;
        let request = &self.admitted.request;
        let mut evidence = HashMap::new();
        let prepared = self.session.prepare_pinned(
            render::Request {
                selected_revision: self.admitted.source.result_version(),
                requested_revision: request.result_revision.as_ref(),
                parent: &request.aggregate_scope_ref,
                basis: request.size_basis.clone(),
                bounds: du_analysis_index::visualization::geometry::Rect {
                    left: request.visible_window.left,
                    top: request.visible_window.top,
                    right: request.visible_window.right,
                    bottom: request.visible_window.bottom,
                },
                format: &request.format,
                strategy: Some(&self.admitted.strategy),
                max_shapes: self.admitted.max_shapes,
            },
            &self.admitted.source,
            |source, item, _| {
                let mut facts = self
                    .producer
                    .child(source, item, request)
                    .map_err(|_| render::Error::InvalidEvidence)?;
                if !self
                    .admitted
                    .facts
                    .classes
                    .contains_key(&facts.presentation.legend_class_key)
                {
                    return Err(render::Error::InvalidEvidence);
                }
                facts.presentation.display_flags.sort_unstable();
                facts.presentation.display_flags.dedup();
                let presentation = ChildPolicyEvidence {
                    path_compare_key: facts.presentation.path_compare_key.clone(),
                    legend_class_key: facts.presentation.legend_class_key.clone(),
                    display_flags: facts.presentation.display_flags.clone(),
                };
                evidence.insert(item.item_no, facts);
                Ok(presentation)
            },
            &mut cancelled,
        );
        match prepared {
            Ok(mut prepared) => {
                prepared.bind_condition(self.admitted.key.clone());
                let reserved = self.session.reserve_prepared(prepared);
                let model = match reserved
                    .preview()
                    .map(|preview| {
                        envelope::build_model(&self.admitted, &preview, &evidence, &mut cancelled)
                    })
                    .transpose()
                {
                    Ok(model) => model.map(Arc::new),
                    Err(_) => return outcome(false, Some("support.internal_error")),
                };
                let candidate = self.session.defer_reserved(reserved);
                match (self.candidate.lock(), self.work.computed.lock()) {
                    (Ok(mut slot), Ok(mut computed)) => {
                        *slot = Some(candidate);
                        computed.wire_model = model;
                        let mut finished = outcome(true, None);
                        finished.counts.completed = evidence.len() as u64;
                        finished
                    }
                    _ => outcome(false, Some("support.internal_error")),
                }
            }
            Err(render::Error::Cancelled) => outcome(false, None),
            Err(_) => outcome(false, Some("support.internal_error")),
        }
    }
}
fn outcome(completed_scope: bool, reason: Option<&str>) -> JobOutcome {
    JobOutcome {
        counts: JobCounts::default(),
        completed_scope,
        failure_reason_key: reason.map(str::to_owned),
        unprocessed_summary: None,
        result_version_id: None,
    }
}
pub(super) fn commit(
    session: &Session,
    work: &Work,
    candidate: &CandidateSlot,
) -> Result<(), String> {
    let members = work.members.lock().map_err(|_| "support.internal_error")?;
    if members.is_empty() {
        return Err("support.internal_error".into());
    }
    let mut slot = candidate.lock().map_err(|_| "support.internal_error")?;
    let mut computed = work.computed.lock().map_err(|_| "support.internal_error")?;
    let deferred = slot.as_mut().ok_or("support.internal_error")?;
    let committed = session
        .commit_deferred(deferred, &mut || false)
        .map_err(|_| "support.internal_error")?;
    computed.committed = Some(committed);
    Ok(())
}
