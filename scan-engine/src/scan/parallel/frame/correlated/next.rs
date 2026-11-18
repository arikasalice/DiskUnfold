use super::{
    Attempt, MAX_EVENT_RESERVATION, MAX_PENDING_ADMITS, MAX_PENDING_NAME_BYTES, PendingAdmit,
    State, Step,
};
use crate::scan::parallel::{
    correlated_budget::{self, MAX_EVENT_PAYLOAD_BYTES},
    frame::{Directory, QueuedEntry},
};
use du_core_contracts::{
    Timestamp,
    scan_observation::{CorrelatedDirectoryEvent, FilterDecision},
};

impl Attempt {
    pub(in crate::scan::parallel) fn next(
        &mut self,
        directory: &Directory,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
        cancelled: &dyn Fn() -> bool,
    ) -> Step {
        if !matches!(self.state, State::Active) || cancelled() {
            self.state = State::Failed;
            return Step::Failed;
        }
        let Some(budget) = self.budget.as_ref().cloned() else {
            self.state = State::Failed;
            return Step::Failed;
        };
        let Some(event_bytes) = MAX_EVENT_RESERVATION else {
            self.state = State::Failed;
            return Step::Failed;
        };
        let Some(permit) = budget.reserve(event_bytes) else {
            // The adapter is already in correlated mode. Stop producing optional
            // evidence, then keep ordinary F02 observations flowing without a clone.
            self.evidence_failed();
            return self.next_without_evidence(directory, include, clock, cancelled);
        };
        self.event_permit = Some(permit);
        let Some(Ok(event)) = directory.with(|dir| dir.next_correlated_event(include, clock))
        else {
            self.state = State::Failed;
            self.event_permit = None;
            return Step::Failed;
        };
        // A cancel during the OS call invalidates the whole attempt, including EOF.
        if cancelled() {
            return self.return_cancelled_event(event);
        }
        self.process_event(event)
    }

    fn return_cancelled_event(&mut self, event: CorrelatedDirectoryEvent) -> Step {
        self.state = State::Failed;
        match event {
            CorrelatedDirectoryEvent::End { .. } | CorrelatedDirectoryEvent::Progress => {
                self.event_permit = None;
                Step::Failed
            }
            event @ CorrelatedDirectoryEvent::Observation { .. } => Step::Returned(event),
            event
                if correlated_budget::event_payload_capacity(&event)
                    .is_some_and(|bytes| bytes <= MAX_EVENT_PAYLOAD_BYTES) =>
            {
                Step::Returned(event)
            }
            _ => {
                self.event_permit = None;
                Step::Failed
            }
        }
    }

    fn process_event(&mut self, event: CorrelatedDirectoryEvent) -> Step {
        match event {
            CorrelatedDirectoryEvent::Progress => {
                self.event_permit = None;
                Step::Progress
            }
            CorrelatedDirectoryEvent::End {
                child_count_exclusive,
            } => {
                if !self.pending_admits.is_empty() {
                    self.state = State::Failed;
                    self.event_permit = None;
                    return Step::Failed;
                }
                self.state = State::Finished;
                self.event_permit = None;
                Step::End {
                    event_watermark_exclusive: self.next_seq,
                    child_count_exclusive,
                }
            }
            payload => {
                if correlated_budget::event_payload_capacity(&payload)
                    .is_none_or(|bytes| bytes > MAX_EVENT_PAYLOAD_BYTES)
                {
                    self.event_permit = None;
                    return match payload {
                        observation @ CorrelatedDirectoryEvent::Observation { .. } => {
                            Step::Returned(observation)
                        }
                        _ => {
                            self.state = State::Failed;
                            Step::Failed
                        }
                    };
                }
                let Some(next) = self.next_seq.checked_add(1) else {
                    self.state = State::Failed;
                    self.event_permit = None;
                    return Step::Failed;
                };
                // Keep the ID with the observation until worker admission is finished.
                let observation = match &payload {
                    CorrelatedDirectoryEvent::Observation { child_id, entry } => {
                        let Some(copy) = correlated_budget::try_clone_timed(entry) else {
                            return Step::Returned(payload);
                        };
                        Some(QueuedEntry {
                            timed: copy,
                            child_id: Some(*child_id),
                            child_attempt: None,
                            payload_permit: None,
                        })
                    }
                    _ => None,
                };
                let callback = match &payload {
                    CorrelatedDirectoryEvent::FilterCallback {
                        child_id,
                        raw_name,
                        decision: FilterDecision::Admit,
                        ..
                    } => Some((*child_id, raw_name)),
                    _ => None,
                };
                if let Some((child_id, raw_name)) = callback {
                    if self
                        .pending_admits
                        .iter()
                        .any(|pending| pending.child_id == child_id)
                        || self.pending_admits.len() == MAX_PENDING_ADMITS
                        || self.pending_name_bytes.saturating_add(raw_name.len())
                            > MAX_PENDING_NAME_BYTES
                    {
                        self.state = State::Failed;
                        self.event_permit = None;
                        return Step::Failed;
                    }
                    let start = self.pending_name_bytes;
                    let Some(end) = start.checked_add(raw_name.len()) else {
                        self.state = State::Failed;
                        self.event_permit = None;
                        return Step::Failed;
                    };
                    self.pending_names[start..end].copy_from_slice(raw_name);
                    self.pending_name_bytes = end;
                    self.pending_admits.push(PendingAdmit {
                        child_id,
                        start,
                        len: raw_name.len(),
                    });
                }
                self.next_seq = next;
                Step::Event {
                    seq: self.next_seq - 1,
                    payload,
                    observation,
                }
            }
        }
    }

    fn next_without_evidence(
        &mut self,
        directory: &Directory,
        include: &mut dyn FnMut(&[u8]) -> FilterDecision,
        clock: &dyn Fn() -> Option<Timestamp>,
        cancelled: &dyn Fn() -> bool,
    ) -> Step {
        if !matches!(self.state, State::Active) || cancelled() {
            self.state = State::Failed;
            return Step::Failed;
        }
        let Some(Ok(event)) = directory.with(|dir| dir.next_correlated_event(include, clock))
        else {
            self.state = State::Failed;
            return Step::Failed;
        };
        if cancelled() {
            self.state = State::Failed;
            return match event {
                observation @ CorrelatedDirectoryEvent::Observation { .. } => {
                    Step::Returned(observation)
                }
                _ => Step::Failed,
            };
        }
        match event {
            CorrelatedDirectoryEvent::Progress => Step::Progress,
            CorrelatedDirectoryEvent::End {
                child_count_exclusive,
            } if self.pending_admits.is_empty() => {
                self.state = State::Finished;
                Step::End {
                    event_watermark_exclusive: self.next_seq,
                    child_count_exclusive,
                }
            }
            CorrelatedDirectoryEvent::End { .. } => {
                self.state = State::Failed;
                Step::Failed
            }
            CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                raw_name,
                decision,
                ..
            } => {
                if decision == FilterDecision::Admit {
                    if self
                        .pending_admits
                        .iter()
                        .any(|pending| pending.child_id == child_id)
                        || self.pending_admits.len() == MAX_PENDING_ADMITS
                        || self.pending_name_bytes.saturating_add(raw_name.len())
                            > MAX_PENDING_NAME_BYTES
                    {
                        self.state = State::Failed;
                        return Step::Failed;
                    }
                    let start = self.pending_name_bytes;
                    let Some(end) = start.checked_add(raw_name.len()) else {
                        self.state = State::Failed;
                        return Step::Failed;
                    };
                    self.pending_names[start..end].copy_from_slice(&raw_name);
                    self.pending_name_bytes = end;
                    self.pending_admits.push(PendingAdmit {
                        child_id,
                        start,
                        len: raw_name.len(),
                    });
                }
                Step::Progress
            }
            CorrelatedDirectoryEvent::Observation { child_id, entry }
                if self.matches_callback(child_id, &entry.entry.name) =>
            {
                Step::Accepted {
                    observation: Some(QueuedEntry {
                        timed: entry,
                        child_id: Some(child_id),
                        child_attempt: None,
                        payload_permit: None,
                    }),
                }
            }
            CorrelatedDirectoryEvent::Observation { .. } => {
                self.state = State::Failed;
                Step::Failed
            }
        }
    }
}
