//! Exact request and job cancellation fences for per-window query regions.
#[cfg(test)]
use super::Current;
use super::{CancelAction, Owner, RegionError, RegionRegistry};
use du_core_contracts::Id;
use std::sync::atomic::Ordering;

impl<K: Copy + Eq> RegionRegistry<K> {
    /// Cancels a synchronous request, or returns its bound job without stopping the permit.
    ///
    /// A caller that receives `Job` must cancel it under the F24 publication gate.
    pub(crate) fn cancel_unbound_request(
        &self,
        owner: &Owner,
        id: &Id,
        sequence: u32,
    ) -> Result<CancelAction, RegionError> {
        let entry = self.lookup(owner, id)?;
        let mut region = entry.lock().map_err(|_| RegionError::Poisoned)?;
        self.check(&region)?;
        let current = region.current.as_mut().ok_or(RegionError::Unavailable)?;
        if current.sequence != sequence {
            return Err(RegionError::Unavailable);
        }
        if let Some(job) = &current.job {
            return Ok(CancelAction::Job(job.clone()));
        }
        if current.finished || current.stopped.swap(true, Ordering::AcqRel) {
            Ok(CancelAction::NoEffect)
        } else {
            Ok(CancelAction::Accepted)
        }
    }

    /// Reads the job bound to one request without changing its cancellation fence.
    pub(crate) fn current_job_for_request(
        &self,
        owner: &Owner,
        id: &Id,
        sequence: u32,
    ) -> Result<Option<Id>, RegionError> {
        let entry = self.lookup(owner, id)?;
        let region = entry.lock().map_err(|_| RegionError::Poisoned)?;
        self.check(&region)?;
        let current = region.current.as_ref().ok_or(RegionError::Unavailable)?;
        if current.sequence != sequence {
            return Err(RegionError::Unavailable);
        }
        Ok(current.job.clone())
    }

    /// Reads the current sequence for an exact job binding without mutating it.
    pub(crate) fn current_sequence_for_job(
        &self,
        owner: &Owner,
        id: &Id,
        job: &Id,
    ) -> Result<u32, RegionError> {
        let entry = self.lookup(owner, id)?;
        let region = entry.lock().map_err(|_| RegionError::Poisoned)?;
        self.check(&region)?;
        region
            .current
            .as_ref()
            .filter(|current| current.job.as_ref() == Some(job))
            .map(|current| current.sequence)
            .ok_or(RegionError::Unavailable)
    }

    /// Verifies and optionally detaches the exact current job while its F24 gate is held.
    pub(crate) fn detach_job_if_current(
        &self,
        owner: &Owner,
        id: &Id,
        sequence: u32,
        job: &Id,
        detach: bool,
    ) -> Result<bool, RegionError> {
        let entry = match self.lookup(owner, id) {
            Ok(entry) => entry,
            Err(RegionError::Unavailable) => return Ok(false),
            Err(error) => return Err(error),
        };
        let mut region = entry.lock().map_err(|_| RegionError::Poisoned)?;
        if region.closed || self.stopped.load(Ordering::Acquire) {
            return Ok(false);
        }
        let Some(current) = region.current.as_mut() else {
            return Ok(false);
        };
        if current.sequence != sequence || current.job.as_ref() != Some(job) {
            return Ok(false);
        }
        if detach {
            current.stopped.store(true, Ordering::Release);
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn job_is_stopped(
        &self,
        owner: &Owner,
        id: &Id,
        sequence: u32,
        job: &Id,
    ) -> Result<bool, RegionError> {
        let entry = self.lookup(owner, id)?;
        let region = entry.lock().map_err(|_| RegionError::Poisoned)?;
        let current = region.current.as_ref().ok_or(RegionError::Unavailable)?;
        if current.sequence != sequence || current.job.as_ref() != Some(job) {
            return Err(RegionError::Unavailable);
        }
        Ok(current.stopped.load(Ordering::Acquire))
    }

    #[cfg(test)]
    pub(crate) fn cancel_request(
        &self,
        owner: &Owner,
        id: &Id,
        sequence: u32,
    ) -> Result<CancelAction, RegionError> {
        self.cancel_matching(owner, id, |current| current.sequence == sequence)
    }

    #[cfg(test)]
    pub(crate) fn cancel_job(
        &self,
        owner: &Owner,
        id: &Id,
        job: &Id,
    ) -> Result<CancelAction, RegionError> {
        self.cancel_matching(owner, id, |current| current.job.as_ref() == Some(job))
    }

    #[cfg(test)]
    fn cancel_matching(
        &self,
        owner: &Owner,
        id: &Id,
        matches: impl FnOnce(&Current) -> bool,
    ) -> Result<CancelAction, RegionError> {
        let entry = self.lookup(owner, id)?;
        let mut region = entry.lock().map_err(|_| RegionError::Poisoned)?;
        self.check(&region)?;
        let current = region.current.as_mut().ok_or(RegionError::Unavailable)?;
        if !matches(current) {
            return Err(RegionError::Unavailable);
        }
        if let Some(job) = &current.job {
            current.stopped.store(true, Ordering::Release);
            return Ok(CancelAction::Job(job.clone()));
        }
        if current.finished || current.stopped.swap(true, Ordering::AcqRel) {
            Ok(CancelAction::NoEffect)
        } else {
            Ok(CancelAction::Accepted)
        }
    }
}
