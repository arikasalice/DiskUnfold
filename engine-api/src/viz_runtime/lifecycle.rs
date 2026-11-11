use super::*;
use du_core_contracts::job::JobState;
use du_job_runtime::{DeduplicationKey, JobPriority, Submission, SubmitResult};
impl Runtime {
    pub(super) fn open(&self) -> Result<Id, EngineError> {
        let _serial = self.serial.lock().map_err(|_| internal())?;
        let channel = self
            .regions
            .open(&self.owner, ())
            .map_err(|_| unavailable())?;
        let mut state = self.state.lock().map_err(|_| internal())?;
        if state.closed {
            return Err(unavailable());
        }
        state.channels.insert(
            channel.clone(),
            ChannelState {
                sequence: 0,
                active: None,
            },
        );
        Ok(channel)
    }
    pub(super) fn render(
        &self,
        admitted: Admitted,
        correlation: Id,
    ) -> Result<VizRenderResponse, EngineError> {
        let admitted = Arc::new(admitted);
        let work = {
            let _serial = self.serial.lock().map_err(|_| internal())?;
            self.check_sequence(&admitted.request)?;
            let existing = self
                .state
                .lock()
                .map_err(|_| internal())?
                .work
                .get(&admitted.key)
                .cloned();
            let joined = existing
                .map(|work| self.join(&admitted.request, work))
                .transpose()?
                .flatten();
            let work = match joined {
                Some(work) => work,
                None => self.submit(admitted.clone(), correlation)?,
            };
            self.replace_subscription(&admitted.request, &work)?;
            work
        };
        self.workers.notify();
        let job = work
            .job
            .lock()
            .map_err(|_| internal())?
            .clone()
            .ok_or_else(internal)?;
        let result = match self
            .assembly
            .jobs
            .wait_viz_terminal(&job, self.assembly.policy.synchronous_wait)
            .map_err(runtime_error)?
        {
            JobState::Done => {
                let (committed, wire_model) = {
                    let computed = work.computed.lock().map_err(|_| internal())?;
                    (
                        computed.committed.clone().ok_or_else(internal)?,
                        computed.wire_model.clone(),
                    )
                };
                match &committed {
                    RuntimeCommitted::Projection(value) => envelope::response(
                        &self.assembly.session,
                        &admitted,
                        value,
                        wire_model.as_deref(),
                    ),
                    RuntimeCommitted::Histogram {
                        fallback,
                        no_attribution,
                    } => super::histogram::response(
                        &self.assembly.session,
                        &admitted,
                        fallback,
                        *no_attribution,
                        wire_model.as_deref(),
                    ),
                    RuntimeCommitted::RankedGroup {
                        fallback,
                        group_ref,
                        row_index,
                    } => super::ranked_group_model::response(
                        &self.assembly.session,
                        &admitted,
                        fallback,
                        group_ref,
                        *row_index,
                        wire_model.as_deref(),
                    ),
                }
            }
            JobState::Failed | JobState::Partial => Err(internal()),
            _ => Ok(envelope::job(&admitted, job)),
        };
        self.check_current(&admitted.request, &work)?;
        result
    }
    fn join(
        &self,
        request: &VizRenderRequest,
        work: Arc<Work>,
    ) -> Result<Option<Arc<Work>>, EngineError> {
        let job = work.job_id()?;
        let joined = self.assembly.jobs.inspect_viz_gate(&job, |state| {
            if state == JobState::CancelRequested {
                return Err(EngineError::reason("viz.busy"));
            }
            if state.is_terminal() {
                return Ok(Some(state));
            }
            Self::stage(request, &work)?;
            Ok(None)
        });
        match joined {
            Err(du_job_runtime::RuntimeError::NotFound) => Ok(None),
            Err(error) => Err(runtime_error(error)),
            Ok(Err(error)) => Err(error),
            Ok(Ok(None)) => Ok(Some(work)),
            Ok(Ok(Some(JobState::Done))) if self.cache_hit(&work)? => {
                // A terminal job cannot publish again. Stage the cached
                // delivery so the channel can adopt it during replacement.
                Self::stage(request, &work)?;
                Ok(Some(work))
            }
            _ => Ok(None),
        }
    }
    fn cache_hit(&self, work: &Work) -> Result<bool, EngineError> {
        let committed = work
            .computed
            .lock()
            .map_err(|_| internal())?
            .committed
            .clone();
        let Some(committed) = committed else {
            return Ok(false);
        };
        let fallback = match committed {
            RuntimeCommitted::Projection(CommittedRender::Model(model)) => model.fallback(),
            RuntimeCommitted::Projection(CommittedRender::Fallback { fallback, .. })
            | RuntimeCommitted::Histogram { fallback, .. }
            | RuntimeCommitted::RankedGroup { fallback, .. } => fallback,
        };
        match self
            .assembly
            .session
            .rows(&du_core_contracts::visualization::rows::VizRowsRequest {
                model_ref: fallback.model_ref,
                result_revision: fallback.result_revision,
                start: 0,
                count: 1,
            }) {
            Ok(_) => Ok(true),
            Err(crate::viz_pages::Error::ModelExpired | crate::viz_pages::Error::StaleVersion) => {
                Ok(false)
            }
            Err(_) => Err(internal()),
        }
    }
    fn check_current(
        &self,
        request: &VizRenderRequest,
        work: &Arc<Work>,
    ) -> Result<(), EngineError> {
        let state = self.state.lock().map_err(|_| internal())?;
        let channel = state
            .channels
            .get(&request.view_channel)
            .ok_or_else(unavailable)?;
        if channel.sequence != request.request_seq.get()
            || channel
                .active
                .as_ref()
                .is_none_or(|active| !Arc::ptr_eq(active, work))
        {
            return Err(unavailable());
        }
        Ok(())
    }
    fn check_sequence(&self, request: &VizRenderRequest) -> Result<(), EngineError> {
        let state = self.state.lock().map_err(|_| internal())?;
        if state.closed {
            return Err(unavailable());
        }
        let channel = state
            .channels
            .get(&request.view_channel)
            .ok_or_else(unavailable)?;
        if request.request_seq.get() <= channel.sequence {
            return Err(invalid());
        }
        Ok(())
    }
    // Stage membership under the new physical gate. The serial lock prevents
    // channel controls until replacement has retired the old membership.
    fn stage(request: &VizRenderRequest, work: &Work) -> Result<(), EngineError> {
        work.members
            .lock()
            .map_err(|_| internal())?
            .insert(request.view_channel.clone());
        Ok(())
    }
    fn submit(&self, admitted: Arc<Admitted>, correlation: Id) -> Result<Arc<Work>, EngineError> {
        self.reclaim(Some(&admitted.key))?;
        self.reserve_work_capacity(&admitted.key)?;
        let work = Arc::new(Work {
            key: admitted.key.clone(),
            job: Mutex::new(None),
            members: Mutex::new(HashSet::new()),
            computed: Mutex::new(Computed::default()),
        });
        let candidate = Arc::new(CandidateSlot::default());
        let body = Arc::new(compute::Compute {
            session: self.assembly.session.clone(),
            producer: self.assembly.producer.clone(),
            admitted: admitted.clone(),
            work: work.clone(),
            candidate: candidate.clone(),
        });
        let session = self.assembly.session.clone();
        let publishing = work.clone();
        let binding = work.clone();
        let result = self
            .assembly
            .jobs
            .submit_viz_with_retained_admission(
                Submission {
                    kind_id: "viz.compute".into(),
                    deduplication_key: Some(DeduplicationKey {
                        scope: self.owner.session.as_str().into(),
                        input: admitted.key.clone(),
                        channel: "gui".into(),
                        key: admitted.key.clone(),
                    }),
                    body,
                    priority: JobPriority::Normal,
                    scope_summary: "visualization".into(),
                    correlation_id: correlation,
                    sample: false,
                },
                Arc::new((candidate.clone(), session.clone(), publishing.clone())),
                move || compute::commit(&session, &publishing, &candidate),
                |job| {
                    let mut bound = binding
                        .job
                        .lock()
                        .map_err(|_| du_job_runtime::RuntimeError::Poisoned)?;
                    let mut members = binding
                        .members
                        .lock()
                        .map_err(|_| du_job_runtime::RuntimeError::Poisoned)?;
                    *bound = Some(job.clone());
                    members.insert(admitted.request.view_channel.clone());
                    Ok(())
                },
            )
            .map_err(runtime_error)?;
        if !matches!(result, SubmitResult::Created(_)) {
            return Err(internal());
        }
        // Workers are notified only after the initial subscriber is visible.
        self.state
            .lock()
            .map_err(|_| internal())?
            .work
            .insert(admitted.key.clone(), work.clone());
        Ok(work)
    }
    fn reserve_work_capacity(&self, key: &str) -> Result<(), EngineError> {
        let candidates = {
            let state = self.state.lock().map_err(|_| internal())?;
            if state.work.contains_key(key)
                || state.work.len() < self.assembly.policy.max_work.get()
            {
                return Ok(());
            }
            state.work.values().cloned().collect::<Vec<_>>()
        };
        for work in candidates {
            let inactive = work.members.lock().map_err(|_| internal())?.is_empty();
            if inactive
                && self
                    .assembly
                    .jobs
                    .state(&work.job_id()?)
                    .map_err(runtime_error)?
                    .is_terminal()
            {
                self.remove_work(&work)?;
                return Ok(());
            }
        }
        Err(EngineError::reason("viz.busy"))
    }
    pub(super) fn reclaim(&self, replacing: Option<&str>) -> Result<(), EngineError> {
        let candidates: Vec<_> = self
            .state
            .lock()
            .map_err(|_| internal())?
            .work
            .values()
            .cloned()
            .collect();
        for work in candidates {
            let job = work
                .job
                .lock()
                .map_err(|_| internal())?
                .clone()
                .ok_or_else(internal)?;
            let state = self.assembly.jobs.state(&job);
            if matches!(state, Err(du_job_runtime::RuntimeError::NotFound)) {
                self.remove_work(&work)?;
                continue;
            }
            let state = state.map_err(runtime_error)?;
            let inactive = work.members.lock().map_err(|_| internal())?.is_empty();
            if state.is_terminal()
                && inactive
                && (replacing == Some(work.key.as_str())
                    || state != JobState::Done
                    || !self.cache_hit(&work)?)
            {
                self.remove_work(&work)?;
            }
        }
        Ok(())
    }
    fn remove_work(&self, work: &Arc<Work>) -> Result<(), EngineError> {
        let retired = self
            .state
            .lock()
            .map_err(|_| internal())?
            .work
            .remove(&work.key);
        drop(retired);
        Ok(())
    }
}
impl Work {
    pub(super) fn job_id(&self) -> Result<Id, EngineError> {
        self.job
            .lock()
            .map_err(|_| internal())?
            .clone()
            .ok_or_else(internal)
    }
}
