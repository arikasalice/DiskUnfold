//! Bounded legacy and pre-metadata filtered enumeration.
use super::*;
use du_core_contracts::scan_observation::TimedFilteredBatch;

impl DirectoryObservations for WindowsDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        Ok(self
            .next_batch_timed(&|| None)?
            .into_iter()
            .map(|item| item.entry)
            .collect())
    }
    fn next_batch_timed(
        &mut self,
        clock: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Result<Vec<TimedObservedEntry>, PortError> {
        if self.filtered == Some(true) {
            return Err(invalid());
        }
        self.filtered = Some(false);
        while self.pending.is_empty() && !self.finished {
            self.fill(&mut |_| FilterDecision::Admit, clock)?;
        }
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
        clock: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Result<TimedFilteredBatch, PortError> {
        if self.filtered == Some(false) {
            return Err(invalid());
        }
        self.filtered = Some(true);
        if self.filter_failed {
            return Ok(TimedFilteredBatch::Undecidable);
        }
        if self.pending.is_empty() && !self.finished {
            self.fill(include, clock)?;
        }
        if self.filter_failed {
            return Ok(TimedFilteredBatch::Undecidable);
        }
        let count = self.pending.len().min(BATCH_SIZE);
        if count > 0 {
            return Ok(TimedFilteredBatch::Entries(
                self.pending.drain(..count).collect(),
            ));
        }
        if self.finished {
            Ok(TimedFilteredBatch::End)
        } else {
            Ok(TimedFilteredBatch::Progress)
        }
    }
}
