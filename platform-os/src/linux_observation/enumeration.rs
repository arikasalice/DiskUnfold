//! Bounded unfiltered and pre-metadata filtered enumeration of one directory.
use super::{
    LinuxDirectory,
    errors::{errno, invalid},
    metadata,
};
use du_core_contracts::{
    Timestamp,
    ports::{PortError, PortErrorKind},
    scan_observation::{
        CorrelatedDirectoryEvent, DirectoryObservations, FilterCallbackPhase, FilterDecision,
        FilteredBatch, ObservedEntry, TimedFilteredBatch, TimedObservedEntry,
    },
};
use rustix::{fs::Dir, io::fcntl_dupfd_cloexec};

/// Directory entries read per bounded step, and the largest batch returned.
pub(super) const BATCH_SIZE: usize = 256;

type Clock<'a> = &'a dyn Fn() -> Option<Timestamp>;

fn correlated_failure() -> PortError {
    PortError {
        kind: PortErrorKind::Corrupt,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

fn correlated_allocation() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

fn correlated_copy_name(name: &[u8]) -> Result<Vec<u8>, PortError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(name.len())
        .map_err(|_| correlated_allocation())?;
    copy.extend_from_slice(name);
    Ok(copy)
}

fn stream_conflict() -> PortError {
    PortError {
        kind: PortErrorKind::Busy,
        reason_key: "os.query_failed".into(),
        os_code: None,
    }
}

impl LinuxDirectory {
    fn fail_correlated<T>(&mut self, error: PortError) -> Result<T, PortError> {
        self.correlated_error = Some(error.clone());
        Err(error)
    }

    /// Reads at most one batch of raw entries and observes only the admitted names.
    fn fill(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: Clock<'_>,
    ) -> Result<(), PortError> {
        if self.entries.is_none() {
            // The duplicate shares the anchor's open-file description and
            // offset. This retained Dir is the only enumerator owned by this
            // LinuxDirectory; metadata and child opens do not advance it.
            let stream = fcntl_dupfd_cloexec(&*self.anchor.fd, 0).map_err(errno)?;
            self.entries = Some(Dir::new(stream).map_err(errno)?);
        }
        let Some(entries) = self.entries.as_mut() else {
            return Err(invalid());
        };
        for _ in 0..BATCH_SIZE {
            let entry = match entries.read() {
                None => {
                    self.finished = true;
                    return Ok(());
                }
                Some(Ok(entry)) => entry,
                Some(Err(error)) => {
                    // Return what was read; the error ends the stream afterwards.
                    self.finished = true;
                    if self.pending.is_empty() {
                        return Err(errno(error));
                    }
                    self.pending_error = Some(errno(error));
                    return Ok(());
                }
            };
            let name = entry.file_name();
            let bytes = name.to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            match include(bytes) {
                FilterDecision::Admit => {
                    let observed = ObservedEntry {
                        name: bytes.to_vec(),
                        enumerated_inode: entry.ino(),
                        enumerated_kind: metadata::kind(Some(entry.file_type())),
                        metadata: metadata::observe(&self.anchor, name),
                    };
                    let observed_at = if observed.metadata.is_ok() {
                        clock()
                    } else {
                        None
                    };
                    self.pending.push_back(TimedObservedEntry {
                        entry: observed,
                        observed_at,
                        placeholder_observation: None,
                    });
                }
                FilterDecision::Exclude => {}
                FilterDecision::Undecidable => {
                    self.pending.clear();
                    self.filter_failed = true;
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Reports a stashed stream error once the entries read before it are drained.
    fn take_error(&mut self) -> Result<(), PortError> {
        match self.pending_error.take() {
            Some(error) if self.pending.is_empty() => Err(error),
            stashed => {
                self.pending_error = stashed;
                Ok(())
            }
        }
    }

    fn correlated_preflight(&mut self) -> Result<Option<CorrelatedDirectoryEvent>, PortError> {
        if !self.correlated {
            return Err(stream_conflict());
        }
        if self.correlated_ended {
            return Err(stream_conflict());
        }
        if let Some(error) = &self.correlated_error {
            return Err(error.clone());
        }
        Ok(self
            .correlated_pending_observation
            .take()
            .map(|(child_id, entry)| CorrelatedDirectoryEvent::Observation { child_id, entry }))
    }

    fn ensure_correlated_entries(&mut self) -> Result<(), PortError> {
        if self.entries.is_none() {
            // Keep one Dir in this object across turns. The duplicate shares
            // the anchor's offset; no other path enumerates it.
            let stream = match fcntl_dupfd_cloexec(&*self.anchor.fd, 0) {
                Ok(stream) => stream,
                Err(error) => return self.fail_correlated(errno(error)),
            };
            let entries = match Dir::new(stream) {
                Ok(entries) => entries,
                Err(error) => return self.fail_correlated(errno(error)),
            };
            self.entries = Some(entries);
        }
        Ok(())
    }

    fn correlated_end(&mut self) -> Result<CorrelatedDirectoryEvent, PortError> {
        self.finished = true;
        let Some(child_count_exclusive) = self.correlated_next_child_id else {
            return self.fail_correlated(correlated_failure());
        };
        self.correlated_ended = true;
        Ok(CorrelatedDirectoryEvent::End {
            child_count_exclusive,
        })
    }

    fn correlated_child_ids(&mut self) -> Result<(u64, u64), PortError> {
        let Some(child_id) = self.correlated_next_child_id else {
            return self.fail_correlated(correlated_failure());
        };
        let Some(next_child_id) = child_id.checked_add(1) else {
            return self.fail_correlated(correlated_failure());
        };
        Ok((child_id, next_child_id))
    }
}

impl DirectoryObservations for LinuxDirectory {
    fn begin_correlated_filtered(&mut self) -> Result<(), PortError> {
        if self.correlated
            || self.filtered.is_some()
            || self.entries.is_some()
            || !self.pending.is_empty()
            || self.pending_error.is_some()
            || self.filter_failed
            || self.finished
        {
            return Err(stream_conflict());
        }
        self.correlated = true;
        self.correlated_next_child_id = Some(0);
        self.correlated_ended = false;
        Ok(())
    }

    fn next_correlated_event(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
    ) -> Result<CorrelatedDirectoryEvent, PortError> {
        if let Some(event) = self.correlated_preflight()? {
            return Ok(event);
        }

        loop {
            self.ensure_correlated_entries()?;
            let next = match self.entries.as_mut() {
                Some(entries) => entries.read(),
                None => return self.fail_correlated(correlated_failure()),
            };
            let entry = match next {
                None => return self.correlated_end(),
                Some(Ok(entry)) => entry,
                Some(Err(error)) => return self.fail_correlated(errno(error)),
            };
            let name = entry.file_name();
            let raw_name = name.to_bytes();
            if raw_name == b"." || raw_name == b".." {
                continue;
            }

            let (child_id, next_child_id) = self.correlated_child_ids()?;
            let callback_name = match correlated_copy_name(raw_name) {
                Ok(name) => name,
                Err(error) => return self.fail_correlated(error),
            };
            // Issue this ID before invoking the filter callback. Any later
            // allocation or observation failure makes the attempt fail, so
            // no End can publish a count that omits this issued ID.
            self.correlated_next_child_id = Some(next_child_id);
            let decision = include(raw_name);
            let observation_name = if decision == FilterDecision::Admit {
                match correlated_copy_name(raw_name) {
                    Ok(name) => Some(name),
                    Err(error) => return self.fail_correlated(error),
                }
            } else {
                None
            };
            let callback = CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                raw_name: callback_name,
                decision,
                phase: FilterCallbackPhase::BeforeMetadata,
            };
            if decision == FilterDecision::Admit {
                let Some(observation_name) = observation_name else {
                    return self.fail_correlated(correlated_failure());
                };
                let observed = ObservedEntry {
                    name: observation_name,
                    enumerated_inode: entry.ino(),
                    enumerated_kind: metadata::kind(Some(entry.file_type())),
                    metadata: metadata::observe(&self.anchor, name),
                };
                let observed_at = if observed.metadata.is_ok() {
                    clock()
                } else {
                    None
                };
                self.correlated_pending_observation = Some((
                    child_id,
                    TimedObservedEntry {
                        entry: observed,
                        observed_at,
                        placeholder_observation: None,
                    },
                ));
            }
            return Ok(callback);
        }
    }

    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(self
            .next_batch_timed(&|| None)?
            .into_iter()
            .map(|item| item.entry)
            .collect())
    }

    fn next_batch_timed(&mut self, clock: Clock<'_>) -> Result<Vec<TimedObservedEntry>, PortError> {
        if self.correlated {
            return Err(stream_conflict());
        }
        if self.filtered == Some(true) {
            return Err(invalid());
        }
        self.filtered = Some(false);
        // Unfiltered reads return full batches; each fill step is still bounded.
        while self.pending.len() < BATCH_SIZE && !self.finished {
            self.fill(&mut |_| FilterDecision::Admit, clock)?;
        }
        self.take_error()?;
        let count = self.pending.len().min(BATCH_SIZE);
        Ok(self.pending.drain(..count).collect())
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        Ok(match self.next_filtered_timed(include, &|| None)? {
            TimedFilteredBatch::Entries(items) => {
                FilteredBatch::Entries(items.into_iter().map(|item| item.entry).collect())
            }
            TimedFilteredBatch::Progress => FilteredBatch::Progress,
            TimedFilteredBatch::End => FilteredBatch::End,
            TimedFilteredBatch::Undecidable => FilteredBatch::Undecidable,
        })
    }

    fn next_filtered_timed(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: Clock<'_>,
    ) -> Result<TimedFilteredBatch, PortError> {
        if self.correlated {
            return Err(stream_conflict());
        }
        if self.filtered == Some(false) {
            return Err(invalid());
        }
        self.filtered = Some(true);
        if !self.filter_failed && self.pending.is_empty() && !self.finished {
            self.fill(include, clock)?;
        }
        if self.filter_failed {
            return Ok(TimedFilteredBatch::Undecidable);
        }
        self.take_error()?;
        let count = self.pending.len().min(BATCH_SIZE);
        if count > 0 {
            return Ok(TimedFilteredBatch::Entries(
                self.pending.drain(..count).collect(),
            ));
        }
        Ok(if self.finished {
            TimedFilteredBatch::End
        } else {
            TimedFilteredBatch::Progress
        })
    }
}
