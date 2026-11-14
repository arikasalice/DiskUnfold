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
            break;
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
            break;
        }
        progress.record(
            event,
            build,
            counts,
            services.context,
            services.ids,
            services.clock,
        )?;
        services.context.report_progress(processed(counts));
    }
    Ok(())
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
            Event::DirectoryCase(root, path, evidence) => {
                build.set_directory_case_evidence(root, &path, evidence)?;
                self.cadence.changed();
                Ok(())
            }
            Event::Item(root_index, path, timed, row) => {
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
                let counted = self.counted(root_index, &path);
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

    /// How the directory of a failed range was counted before the failure.
    ///
    /// A directory below a root was counted as completed when it was
    /// enumerated. A root was counted when it was admitted; its first failure
    /// moves it to failed unless it was already counted as failed.
    fn counted(&mut self, root_index: u32, path: &[Vec<u8>]) -> record::Counted {
        if !path.is_empty() {
            return record::Counted::Completed;
        }
        let earlier = usize::try_from(root_index)
            .ok()
            .and_then(|index| self.counted_roots.get_mut(index))
            .is_some_and(|failed| std::mem::replace(failed, true));
        if earlier {
            record::Counted::Failed
        } else {
            record::Counted::Completed
        }
    }
}

pub(super) fn reason(error: &PortError) -> &'static str {
    super::super::issues::reason(error)
}
