//! Bounded legacy and pre-metadata filtered enumeration.
use super::*;

impl DirectoryObservations for WindowsDirectory {
    fn next_batch(&mut self) -> Result<Vec<ObservedEntry>, PortError> {
        if self.filtered == Some(true) {
            return Err(invalid());
        }
        self.filtered = Some(false);
        while self.pending.is_empty() && !self.finished {
            self.fill(&mut |_| FilterDecision::Admit)?;
        }
        let count = self.pending.len().min(BATCH_SIZE);
        Ok(self.pending.drain(..count).collect())
    }

    fn next_filtered(
        &mut self,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
    ) -> Result<FilteredBatch, PortError> {
        if self.filtered == Some(false) {
            return Err(invalid());
        }
        self.filtered = Some(true);
        if self.filter_failed {
            return Ok(FilteredBatch::Undecidable);
        }
        if self.pending.is_empty() && !self.finished {
            self.fill(include)?;
        }
        if self.filter_failed {
            return Ok(FilteredBatch::Undecidable);
        }
        let count = self.pending.len().min(BATCH_SIZE);
        if count > 0 {
            return Ok(FilteredBatch::Entries(
                self.pending.drain(..count).collect(),
            ));
        }
        if self.finished {
            Ok(FilteredBatch::End)
        } else {
            Ok(FilteredBatch::Progress)
        }
    }
}
