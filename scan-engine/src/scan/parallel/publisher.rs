//! Timed publication of new scan observations.
use super::*;
use du_core_contracts::job::JobProgress;

pub(super) struct Services<'a> {
    pub context: &'a dyn JobContext,
    pub ids: &'a dyn ResultIds,
    pub clock: &'a dyn MonotonicClock,
    pub abort: &'a AtomicBool,
}

pub(super) fn consume(
    receiver: mpsc::Receiver<Event>,
    progress: &mut Progress,
    build: &mut BuildHandle,
    counts: &mut JobCounts,
    services: Services<'_>,
) -> Result<(), BuildError> {
    loop {
        if services.abort.load(Ordering::Acquire) {
            return drain_aborted(receiver, progress, build, counts, &services);
        }
        let wait = if services.context.checkpoint() == Checkpoint::Cancel {
            None
        } else {
            services
                .clock
                .now()
                .and_then(|now| progress.cadence.remaining(now))
        };
        if wait == Some(Duration::ZERO) {
            progress.publish(build, services.context, services.ids, services.clock)?;
            continue;
        }
        let event = match wait {
            Some(wait) => match receiver.recv_timeout(wait) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match receiver.recv() {
                Ok(event) => event,
                Err(_) => break,
            },
        };
        if services.abort.load(Ordering::Acquire) {
            return drain_aborted(receiver, progress, build, counts, &services);
        }
        if let Err(error) = progress.record(
            event,
            build,
            counts,
            services.context,
            services.ids,
            services.clock,
        ) {
            services.abort.store(true, Ordering::Release);
            services.context.abort_flows();
            let _ = drain_aborted(receiver, progress, build, counts, &services);
            return Err(error);
        }
        services.context.report_progress(processed(counts));
    }
    Ok(())
}

/// On internal abort, keep the publisher alive until workers release their senders.
/// Item observations and failure marks are retained; correlated evidence and EOF are refused.
fn drain_aborted(
    receiver: mpsc::Receiver<Event>,
    progress: &mut Progress,
    build: &mut BuildHandle,
    counts: &mut JobCounts,
    services: &Services<'_>,
) -> Result<(), BuildError> {
    let mut first_error = None;
    while let Ok(event) = receiver.recv() {
        match event {
            Event::FolderAttemptBegin { reply, _permit, .. } => {
                let _ = reply.send(Err(du_analysis_index::FolderAttemptError::CapacityExceeded));
            }
            Event::FolderAttemptEvent { reply, .. } => {
                if let Some(reply) = reply {
                    let _ =
                        reply.send(Err(du_analysis_index::FolderAttemptError::CapacityExceeded));
                }
            }
            Event::FolderAttemptEnd { reply, .. } => {
                let _ = reply.send(Err(du_analysis_index::FolderAttemptError::Finished));
            }
            Event::RootBatch(..) => {
                // Abort draining must not publish a new result version.
            }
            Event::CorrelatedItem(root, path, timed, admission, permit) => {
                let event = Event::Item(root, path, timed, admission, Some(permit));
                if let Err(error) = progress.record(
                    event,
                    build,
                    counts,
                    services.context,
                    services.ids,
                    services.clock,
                ) {
                    first_error.get_or_insert(error);
                }
            }
            event => {
                if let Err(error) = progress.record(
                    event,
                    build,
                    counts,
                    services.context,
                    services.ids,
                    services.clock,
                ) {
                    first_error.get_or_insert(error);
                }
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// F24 item progress: every recorded item, failures included; the total stays
/// unknown while the tree is being enumerated.
fn processed(counts: &JobCounts) -> JobProgress {
    JobProgress {
        completed: counts.completed.saturating_add(counts.failed),
        total: None,
    }
}

impl Progress {
    /// Keep observations and explicit disconnection evidence even without a successful root batch.
    pub(super) fn has_usable_result(&self, disconnects: &disconnect::Disconnects) -> bool {
        self.failed_roots.contains(&false) || disconnects.any_closed()
    }

    pub(super) fn record(
        &mut self,
        event: Event,
        build: &mut BuildHandle,
        counts: &mut JobCounts,
        context: &dyn JobContext,
        ids: &dyn ResultIds,
        clock: &dyn MonotonicClock,
    ) -> Result<(), BuildError> {
        match event {
            Event::FolderAttemptBegin {
                root_index,
                path,
                attempt_id,
                reply,
                _permit,
            } => {
                let result = if rejects_child_attempt(&path) {
                    Err(du_analysis_index::FolderAttemptError::CapacityExceeded)
                } else {
                    build.begin_folder_attempt(root_index, &path, attempt_id)
                };
                let _ = reply.send(result);
                Ok(())
            }
            Event::FolderAttemptEvent {
                token,
                seq,
                input,
                reply,
                _permit,
            } => {
                let result = build.record_folder_attempt_event(token, seq, input);
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
                Ok(())
            }
            Event::FolderAttemptEnd {
                token,
                event_watermark_exclusive,
                child_count_exclusive,
                reply,
            } => {
                let result = build.end_folder_attempt(
                    token,
                    event_watermark_exclusive,
                    child_count_exclusive,
                );
                let _ = reply.send(result);
                Ok(())
            }
            Event::DirectoryCase(root, path, evidence) => {
                build.set_directory_case_evidence(root, &path, evidence)?;
                self.cadence.changed();
                Ok(())
            }
            Event::Excluded {
                root_index,
                path,
                count,
                complete,
            } => {
                build.set_excluded_children(root_index, &path, count, complete)?;
                self.cadence.changed();
                Ok(())
            }
            Event::Unread(root_index, path) => {
                self.counted_roots.unread(root_index, &path);
                build.set_directory_unread(root_index, &path, UnreadReason::Cancelled)?;
                self.cadence.changed();
                Ok(())
            }
            Event::Item(root_index, path, timed, row, _permit) => {
                let gap_at = || context.utc_now();
                let item = (root_index, path, &*timed);
                record::record_item(build, counts, &gap_at, item, row)?;
                self.cadence.changed();
                Ok(())
            }
            Event::CorrelatedItem(root_index, path, timed, row, _permit) => {
                let gap_at = || context.utc_now();
                let item = (root_index, path, &*timed);
                record::record_item(build, counts, &gap_at, item, row)?;
                self.cadence.changed();
                Ok(())
            }
            Event::Undecided(root_index, path) => {
                // The source supplied no observation time for a withheld name.
                let unread = admission::Unread::Unverifiable;
                admission::record_unread(build, (root_index, path), unread, counts, None)?;
                self.cadence.changed();
                Ok(())
            }
            Event::Failure {
                root_index,
                path,
                root,
                reason,
                stage,
                observed_at,
            } => {
                if root && !self.root_published[root_index as usize] {
                    self.failed_roots[root_index as usize] = true;
                }
                let counted = self.counted_roots.counted(root_index, &path);
                let times = (observed_at, context.utc_now());
                let range = (root_index, path);
                record::record_failure(build, counts, range, (stage, reason), times, counted)?;
                self.cadence.changed();
                Ok(())
            }
            Event::RootBatch(root_index, modified_at, observed_at) => {
                let has_timestamp = modified_at.is_some() || observed_at.is_some();
                if let Some(time) = observed_at {
                    build.set_item_observed_at(root_index, &[], time)?;
                }
                if let Some(time) = modified_at {
                    build.set_modified_at(root_index, &[], time)?;
                }
                if has_timestamp {
                    self.cadence.changed();
                }
                self.root_published[root_index as usize] = true;
                if !self.published && context.checkpoint() != Checkpoint::Cancel {
                    self.publish(build, context, ids, clock)?;
                }
                Ok(())
            }
        }
    }

    pub(super) fn publish(
        &mut self,
        build: &mut BuildHandle,
        context: &dyn JobContext,
        ids: &dyn ResultIds,
        clock: &dyn MonotonicClock,
    ) -> Result<(), BuildError> {
        let started = clock.now();
        build.publish_partial_at(ids.next_id(), context.utc_now())?;
        let finished = clock.now();
        if let (Some(started), Some(finished)) = (started, finished) {
            self.cadence.published(started, finished);
        }
        self.published = true;
        Ok(())
    }
}

pub(super) fn reason(error: &PortError) -> &'static str {
    super::super::issues::reason(error)
}

/// Test switch that makes F03 refuse child attempts, as it does after discarding evidence.
#[cfg(test)]
std::thread_local! {
    pub(super) static REJECT_CHILD_ATTEMPTS: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
fn rejects_child_attempt(path: &[Vec<u8>]) -> bool {
    !path.is_empty() && REJECT_CHILD_ATTEMPTS.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn rejects_child_attempt(_: &[Vec<u8>]) -> bool {
    false
}
