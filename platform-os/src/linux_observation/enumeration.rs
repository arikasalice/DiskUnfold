//! Bounded unfiltered and pre-metadata filtered enumeration of one directory.
use super::{
    LinuxDirectory,
    errors::{errno, invalid},
    metadata,
};
use du_core_contracts::{
    Timestamp,
    ports::PortError,
    scan_observation::{
        DirectoryObservations, FilterDecision, FilteredBatch, ObservedEntry, TimedFilteredBatch,
        TimedObservedEntry,
    },
};
use rustix::{fs::Dir, io::fcntl_dupfd_cloexec};

/// Directory entries read per bounded step, and the largest batch returned.
pub(super) const BATCH_SIZE: usize = 256;

type Clock<'a> = &'a dyn Fn() -> Option<Timestamp>;

impl LinuxDirectory {
    /// Reads at most one batch of raw entries and observes only the admitted names.
    fn fill(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: Clock<'_>,
    ) -> Result<(), PortError> {
        if self.entries.is_none() {
            // A duplicate keeps the stream position away from the anchor handle.
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
}

impl DirectoryObservations for LinuxDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(self
            .next_batch_timed(&|| None)?
            .into_iter()
            .map(|item| item.entry)
            .collect())
    }

    fn next_batch_timed(&mut self, clock: Clock<'_>) -> Result<Vec<TimedObservedEntry>, PortError> {
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
