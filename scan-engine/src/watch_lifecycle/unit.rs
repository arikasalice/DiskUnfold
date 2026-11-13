use super::model::{Action, Capability, Definition, Error, Registration, State, Status, Support};
use crate::watch_coalescer::{
    Batch, Coalescer, Config, Context, Cursor, Gap, GapReason, Key, Trust,
};

#[derive(Clone)]
pub(super) struct Unit {
    pub definition: Definition,
    pub state: State,
    pub generation: u64,
    pub token: u64,
    pub attempt_generation: u64,
    pub sequence: u64,
    pub change_sequence: u64,
    pub last_gap: u64,
    pub gaps: Vec<Gap>,
    pub last_observed: Option<u64>,
    pub last_time: u64,
    pub capability: Capability,
    pub handle: Option<u64>,
    pub remove_pending: bool,
    pub remove_failed: bool,
    pub stream: Option<Coalescer>,
    pub config: Config,
}
impl Unit {
    pub fn new(
        definition: Definition,
        capability: Capability,
        attempt: (u64, u64),
        config: Config,
    ) -> Self {
        let (token, at) = attempt;
        Self {
            definition,
            state: State::Starting,
            generation: 0,
            token,
            attempt_generation: 1,
            sequence: 0,
            change_sequence: 0,
            last_gap: 0,
            gaps: Vec::new(),
            last_observed: None,
            last_time: at,
            capability,
            handle: None,
            remove_pending: false,
            remove_failed: false,
            stream: None,
            config,
        }
    }
    pub(super) fn check_time(&self, at: u64) -> Result<(), Error> {
        if at < self.last_time {
            Err(Error::ClockRegressed)
        } else {
            Ok(())
        }
    }
    pub(super) fn check_sequences(&self, count: usize) -> Result<(), Error> {
        self.sequence
            .checked_add(u64::try_from(count).map_err(|_| Error::CounterExhausted)?)
            .ok_or(Error::CounterExhausted)?;
        Ok(())
    }
    pub(super) fn check_changes(&self, count: usize) -> Result<(), Error> {
        self.change_sequence
            .checked_add(u64::try_from(count).map_err(|_| Error::CounterExhausted)?)
            .ok_or(Error::CounterExhausted)?;
        Ok(())
    }
    pub(super) fn check_gaps(&self, count: usize) -> Result<(), Error> {
        if self
            .gaps
            .len()
            .checked_add(count)
            .is_none_or(|n| n > self.config.gap_limit)
        {
            return Err(Error::GapCapacity);
        }
        self.last_gap
            .checked_add(u64::try_from(count).map_err(|_| Error::CounterExhausted)?)
            .ok_or(Error::CounterExhausted)?;
        Ok(())
    }
    pub(super) fn status(&mut self, state: State, reason: Option<&'static str>, at: u64) -> Action {
        self.sequence += 1;
        let previous = Some(self.state);
        self.state = state;
        self.last_time = at;
        Action::Status(Status {
            watch_ref: self.definition.watch_ref,
            state,
            previous,
            reason,
            generation: self.generation,
            sequence: self.sequence,
            observed_at: at,
            last_observed: self.last_observed,
        })
    }
    pub fn initial(&mut self, at: u64) -> Vec<Action> {
        self.sequence = 1;
        vec![
            Action::Status(Status {
                watch_ref: self.definition.watch_ref,
                state: State::Starting,
                previous: None,
                reason: None,
                generation: 0,
                sequence: 1,
                observed_at: at,
                last_observed: None,
            }),
            Action::Register {
                token: self.token,
                generation: self.attempt_generation,
                definition: self.definition.clone(),
            },
        ]
    }
    pub fn unsupported(&mut self, at: u64, reason: &'static str) -> Vec<Action> {
        self.sequence = 1;
        self.state = State::Unsupported;
        vec![Action::Status(Status {
            watch_ref: self.definition.watch_ref,
            state: State::Unsupported,
            previous: None,
            reason: Some(reason),
            generation: 0,
            sequence: 1,
            observed_at: at,
            last_observed: None,
        })]
    }
    pub fn check_retry(&self, at: u64, status_count: usize) -> Result<(), Error> {
        self.check_time(at)?;
        if !matches!(
            self.state,
            State::Stopped | State::Lost | State::Unsupported
        ) || self.handle.is_some()
            || self.remove_pending
        {
            return Err(Error::InvalidState);
        }
        self.generation
            .checked_add(1)
            .ok_or(Error::CounterExhausted)?;
        self.check_sequences(status_count)
    }
    pub fn retry(
        &mut self,
        capability: Capability,
        token: u64,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        let unsupported = capability.notification == Support::Unsupported;
        self.check_retry(at, if unsupported { 2 } else { 1 })?;
        self.token = token;
        self.attempt_generation = self.generation + 1;
        self.capability = capability;
        let mut result = vec![self.status(State::Starting, None, at)];
        if unsupported {
            result.push(self.status(State::Unsupported, Some("watch.unsupported"), at));
        } else {
            result.push(Action::Register {
                token,
                generation: self.attempt_generation,
                definition: self.definition.clone(),
            });
        }
        Ok(result)
    }
    pub fn retry_at_capacity(&mut self, at: u64) -> Result<Vec<Action>, Error> {
        self.check_retry(at, 2)?;
        Ok(vec![
            self.status(State::Starting, None, at),
            self.status(State::Unsupported, Some("watch.resource_limit"), at),
        ])
    }
    pub(super) fn context(&self, generation: u64) -> Context {
        Context {
            watch_ref: self.definition.watch_ref,
            scope_revision: self.definition.scope_revision,
            volume: self.definition.volume.clone(),
            generation,
        }
    }
    pub(super) fn gap(
        id: u64,
        reason: GapReason,
        path: Option<Key>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Gap {
        Gap {
            id,
            reason,
            path,
            start,
            end,
        }
    }
    pub(super) fn batch(&self, sequence: u64, generation: u64, gaps: Vec<Gap>, at: u64) -> Action {
        Action::Changes(Batch {
            context: self.context(generation),
            sequence,
            change_sequence: self.change_sequence + 1,
            replaced_gap_ids: Vec::new(),
            interval_start: at,
            interval_end: at,
            changes: Vec::new(),
            gaps,
            trust: Trust::Gap,
        })
    }
    pub fn registration(
        &mut self,
        token: u64,
        generation: u64,
        outcome: &Registration,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        if token != self.token
            || generation != self.attempt_generation
            || !matches!(self.state, State::Starting | State::Stopping)
            || self.remove_pending
        {
            return Ok(Vec::new());
        }
        self.check_time(at)?;
        if self.state == State::Stopping {
            return self.registration_after_stop(outcome, at);
        }
        match outcome {
            Registration::Acquired { handle, uncovered } => self.activate(*handle, uncovered, at),
            Registration::ResourceLimit => {
                self.check_sequences(1)?;
                Ok(vec![self.status(
                    State::Unsupported,
                    Some("watch.resource_limit"),
                    at,
                )])
            }
            Registration::Disconnected | Registration::AccessLost | Registration::Other => {
                self.registration_lost(outcome, at)
            }
        }
    }
    fn registration_after_stop(
        &mut self,
        outcome: &Registration,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        match outcome {
            Registration::Acquired { handle, .. } => {
                self.handle = Some(*handle);
                self.remove_pending = true;
                self.last_time = at;
                Ok(vec![Action::Remove {
                    token: self.token,
                    generation: self.attempt_generation,
                    handle: *handle,
                }])
            }
            _ => {
                self.check_sequences(1)?;
                Ok(vec![self.status(State::Stopped, Some("watch.stopped"), at)])
            }
        }
    }
    fn registration_lost(&mut self, outcome: &Registration, at: u64) -> Result<Vec<Action>, Error> {
        self.check_sequences(1)?;
        // Preserve one additional slot so a later stop can always fence input.
        if let Err(error) = self.check_gaps(2) {
            if error == Error::GapCapacity {
                let mut trial = self.clone();
                if let Some(mut actions) = trial.compact_for_capacity(at)? {
                    actions.extend(trial.registration_lost(outcome, at)?);
                    *self = trial;
                    return Ok(actions);
                }
            }
            return Err(error);
        }
        self.last_gap += 1;
        self.gaps.push(Self::gap(
            self.last_gap,
            GapReason::Unavailable,
            None,
            self.last_observed.or(self.definition.baseline_end),
            None,
        ));
        let reason = match outcome {
            Registration::Disconnected => "watch.disconnected",
            Registration::AccessLost => "watch.access_lost",
            _ => "watch.unclassified",
        };
        Ok(vec![self.status(State::Lost, Some(reason), at)])
    }
    fn activate(&mut self, handle: u64, uncovered: &[Key], at: u64) -> Result<Vec<Action>, Error> {
        let degraded = self.capability.degraded() || !uncovered.is_empty();
        let all = self.capability.gap_for_all() || (degraded && uncovered.is_empty());
        let partial_count = if all { 1 } else { uncovered.len() };
        let new_count = usize::from(self.generation == 0 && self.gaps.is_empty()) + partial_count;
        // Reserve separate slots for loss of observation and later user stop.
        if let Err(error) = self.check_gaps(new_count + 2) {
            if error == Error::GapCapacity {
                let mut trial = self.clone();
                if let Some(mut actions) = trial.compact_for_capacity(at)? {
                    actions.extend(trial.activate(handle, uncovered, at)?);
                    *self = trial;
                    return Ok(actions);
                }
            }
            return Err(error);
        }
        self.check_sequences(2)?;
        self.check_changes(1)?;
        let generation = self.attempt_generation;
        let mut gaps = self.gaps.clone();
        let mut id = self.last_gap;
        let mut changed: Vec<Gap> = gaps
            .iter_mut()
            .filter(|g| g.end.is_none())
            .map(|g| {
                g.end = Some(at);
                g.clone()
            })
            .collect();
        if self.generation == 0 && !gaps.is_empty() {
            changed = gaps.clone();
        }
        if self.generation == 0 && gaps.is_empty() {
            id += 1;
            let gap = Self::gap(
                id,
                GapReason::BeforeStart,
                None,
                self.definition.baseline_end,
                Some(at),
            );
            gaps.push(gap.clone());
            changed.push(gap);
        }
        if all {
            id += 1;
            let gap = Self::gap(id, GapReason::PartialCapability, None, Some(at), None);
            gaps.push(gap.clone());
            changed.push(gap);
        } else {
            for path in uncovered {
                id += 1;
                let gap = Self::gap(
                    id,
                    GapReason::PartialCapability,
                    Some(path.clone()),
                    Some(at),
                    None,
                );
                gaps.push(gap.clone());
                changed.push(gap);
            }
        }
        let open = gaps.iter().filter(|g| g.end.is_none()).cloned().collect();
        let stream = Coalescer::new(
            self.context(generation),
            self.config,
            Cursor {
                sequence: self.sequence + 2,
                change_sequence: self.change_sequence + 1,
                last_gap_id: id,
            },
            open,
        )?;
        let batch = self.batch(self.sequence + 1, generation, changed, at);
        self.gaps = gaps;
        self.last_gap = id;
        self.generation = generation;
        self.handle = Some(handle);
        self.last_observed = Some(at);
        self.sequence += 1;
        self.change_sequence += 1;
        self.stream = Some(stream);
        let state = if degraded {
            State::Degraded
        } else {
            State::Active
        };
        let status = self.status(
            state,
            if degraded {
                Some("watch.partial")
            } else {
                None
            },
            at,
        );
        Ok(vec![batch, status])
    }
}
