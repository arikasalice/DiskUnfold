use crate::{BuildError, BuildHandle};

impl BuildHandle {
    /// Sets the identity-table capacity before collecting observations.
    ///
    /// The caller is responsible for validating a configured value. Publications
    /// retain this capacity for their lifetime, including lazy detail queries.
    pub fn set_shared_table_limit(&mut self, limit: usize) -> Result<(), BuildError> {
        if self.finished || self.published || !self.observations.is_empty() {
            return Err(BuildError::Unavailable);
        }
        self.shared_table_limit = limit;
        Ok(())
    }
}
