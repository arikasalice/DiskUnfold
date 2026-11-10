//! Timed publication of new scan observations.
use super::*;

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
    }
    Ok(())
}

impl Progress {
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
            Event::Item(root_index, path, timed) => {
                let entry = &timed.entry;
                let gap = entry_gap(entry);
                record_at(build, root_index, entry, &path, counts)?;
                if gap.is_none()
                    && let Some(time) = &timed.observed_at
                {
                    build.set_item_observed_at(root_index, &path, time.clone())?;
                }
                if let Some(reason) = gap {
                    build.record_gap_at(root_index, path, reason.into(), context.utc_now())?;
                }
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
                counts.failed += 1;
                if root && !self.root_published[root_index as usize] {
                    self.failed_roots[root_index as usize] = true;
                }
                build.record_scan_issue(du_core_contracts::scan_failures::ScanIssue {
                    root_index,
                    relative: path.clone(),
                    kind: du_core_contracts::scan_failures::ScanFailureKind::UnreadRange,
                    stage,
                    reason_key: reason.into(),
                    observed_at: observed_at.clone(),
                })?;
                build.record_gap_at(root_index, path, reason.into(), context.utc_now())?;
                self.cadence.changed();
                Ok(())
            }
            Event::RootBatch(root_index, modified_at, observed_at) => {
                if let Some(time) = observed_at {
                    build.set_item_observed_at(root_index, &[], time)?;
                }
                if let Some(time) = modified_at {
                    build.set_modified_at(root_index, &[], time)?;
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
    match error.kind {
        PortErrorKind::PermissionDenied => "scan.path_denied",
        PortErrorKind::NotFound => "scan.path_vanished",
        _ => "scan.unclassified",
    }
}

fn entry_gap(entry: &ObservedEntry) -> Option<&'static str> {
    match &entry.metadata {
        Err(error) => Some(reason(error)),
        Ok(meta)
            if entry.enumerated_inode == 0
                || entry.enumerated_inode != meta.inode
                || (entry.enumerated_kind != ObservedKind::Unknown
                    && entry.enumerated_kind != meta.kind) =>
        {
            Some("scan.unclassified")
        }
        Ok(meta) if matches!(meta.kind, ObservedKind::Unknown | ObservedKind::Other) => {
            Some("scan.unclassified")
        }
        Ok(meta) if meta.kind == ObservedKind::File => meta.size.as_ref().err().map(reason),
        _ => None,
    }
}
