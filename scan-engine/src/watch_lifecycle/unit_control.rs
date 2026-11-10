use super::model::{Action, Completion, Error, Interruption, State, Support};
use super::unit::Unit;
use crate::watch_coalescer::{ChangeKind, GapReason, Key, Signal};

impl Unit {
    pub fn signal(
        &mut self,
        token: u64,
        generation: u64,
        signal: &Signal,
    ) -> Result<Vec<Action>, Error> {
        if token != self.token
            || generation != self.generation
            || !matches!(self.state, State::Active | State::Degraded)
        {
            return Ok(Vec::new());
        }
        self.check_time(signal.observed_at)?;
        // A new overflow must leave room for both an interruption and a stop.
        // If that cannot be guaranteed, mark the whole unit unavailable before
        // dropping this signal; the open gap covers it and every later signal.
        if let Err(error) = self.check_gaps(3) {
            if error == Error::GapCapacity {
                let mut trial = self.clone();
                if let Some(mut actions) = trial.compact_for_capacity(signal.observed_at)? {
                    actions.extend(trial.signal(token, generation, signal)?);
                    *self = trial;
                    return Ok(actions);
                }
            }
            return self.interrupted(token, generation, Interruption::Other, signal.observed_at);
        }
        let fallback;
        let signal = if self.capability.path == Support::Unsupported {
            fallback = Signal {
                path: None,
                directory_subtree: true,
                kind: ChangeKind::Unknown,
                observed_at: signal.observed_at,
            };
            &fallback
        } else {
            signal
        };
        let stream = self.stream.as_mut().ok_or(Error::InvalidState)?;
        let batches = stream.signal(generation, signal)?;
        self.sequence = stream.sequence();
        self.change_sequence = stream.change_sequence();
        self.last_gap = stream.last_gap_id();
        for gap in batches.iter().flat_map(|b| &b.gaps) {
            if !self.gaps.iter().any(|old| old.id == gap.id) {
                self.gaps.push(gap.clone());
            }
        }
        self.last_time = signal.observed_at;
        self.last_observed = Some(signal.observed_at);
        Ok(batches.into_iter().map(Action::Changes).collect())
    }
    pub fn signal_lost(
        &mut self,
        token: u64,
        generation: u64,
        path: Option<&Key>,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        if token != self.token
            || generation != self.generation
            || !matches!(self.state, State::Active | State::Degraded)
        {
            return Ok(Vec::new());
        }
        self.check_time(at)?;
        // A precise loss gap is optional only if a wider open gap replaces it.
        if let Err(error) = self.check_gaps(3) {
            if error == Error::GapCapacity {
                let mut trial = self.clone();
                if let Some(mut actions) = trial.compact_for_capacity(at)? {
                    actions.extend(trial.signal_lost(token, generation, path, at)?);
                    *self = trial;
                    return Ok(actions);
                }
            }
            return self.interrupted(token, generation, Interruption::Other, at);
        }
        let stream = self.stream.as_mut().ok_or(Error::InvalidState)?;
        let path = if self.capability.path == Support::Unsupported {
            None
        } else {
            path
        };
        let batches = stream.signal_lost(generation, path, at)?;
        self.sequence = stream.sequence();
        self.change_sequence = stream.change_sequence();
        self.last_gap = stream.last_gap_id();
        for gap in batches.iter().flat_map(|batch| &batch.gaps) {
            if !self.gaps.iter().any(|old| old.id == gap.id) {
                self.gaps.push(gap.clone());
            }
        }
        self.last_time = at;
        Ok(batches.into_iter().map(Action::Changes).collect())
    }
    pub fn tick(&mut self, token: u64, generation: u64, at: u64) -> Result<Vec<Action>, Error> {
        if token != self.token
            || generation != self.generation
            || !matches!(self.state, State::Active | State::Degraded)
        {
            return Ok(Vec::new());
        }
        self.check_time(at)?;
        let stream = self.stream.as_mut().ok_or(Error::InvalidState)?;
        let batches = stream.tick(generation, at)?;
        self.sequence = stream.sequence();
        self.change_sequence = stream.change_sequence();
        self.last_time = at;
        Ok(batches.into_iter().map(Action::Changes).collect())
    }
    pub fn interrupted(
        &mut self,
        token: u64,
        generation: u64,
        reason: Interruption,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        if token != self.token
            || generation != self.generation
            || !matches!(self.state, State::Active | State::Degraded)
        {
            return Ok(Vec::new());
        }
        self.check_time(at)?;
        self.check_gaps(2)?;
        let buffered = self
            .stream
            .as_ref()
            .ok_or(Error::InvalidState)?
            .buffered_len();
        self.check_sequences(buffered.div_ceil(32) + 2)?;
        self.check_changes(buffered.div_ceil(32) + 1)?;
        let stream = self.stream.as_mut().ok_or(Error::InvalidState)?;
        let batches = stream.flush(generation, at)?;
        let gap_seq = stream.reserve_status_sequence()?;
        let status_seq = stream.reserve_status_sequence()?;
        let gap_change_seq = stream.reserve_change_sequence()?;
        let mut out: Vec<_> = batches.into_iter().map(Action::Changes).collect();
        let mut changed: Vec<_> = self
            .gaps
            .iter_mut()
            .filter(|g| g.reason == GapReason::PartialCapability && g.end.is_none())
            .map(|g| {
                g.end = Some(at);
                g.clone()
            })
            .collect();
        self.last_gap += 1;
        let gap = Self::gap(
            self.last_gap,
            GapReason::Unavailable,
            None,
            self.last_observed,
            None,
        );
        self.gaps.push(gap.clone());
        changed.push(gap);
        self.change_sequence = gap_change_seq - 1;
        out.push(self.batch(gap_seq, generation, changed, at));
        self.change_sequence = gap_change_seq;
        self.sequence = status_seq - 1;
        let key = match reason {
            Interruption::Disconnected => "watch.disconnected",
            Interruption::AccessLost => "watch.access_lost",
            Interruption::Other => "watch.unclassified",
        };
        out.push(self.status(State::Lost, Some(key), at));
        self.stream = None;
        Ok(out)
    }
    pub fn stop(&mut self, at: u64) -> Result<Vec<Action>, Error> {
        if matches!(self.state, State::Stopping | State::Stopped) {
            return Ok(Vec::new());
        }
        if !matches!(
            self.state,
            State::Starting | State::Active | State::Degraded | State::Lost
        ) {
            return Err(Error::InvalidState);
        }
        self.check_time(at)?;
        if matches!(self.state, State::Active | State::Degraded) {
            self.stop_active(at)
        } else {
            self.stop_inactive(at)
        }
    }
    fn stop_inactive(&mut self, at: u64) -> Result<Vec<Action>, Error> {
        let before = self.generation == 0 && self.gaps.is_empty();
        let already_open = self
            .gaps
            .iter()
            .any(|gap| gap.end.is_none() && gap.reason == GapReason::Stopped);
        let count = usize::from(before) + usize::from(!already_open);
        self.check_gaps(count)?;
        let emit_gap = self.generation > 0 && !already_open;
        let immediate_stopped = self.state == State::Lost && self.handle.is_none();
        self.check_sequences(1 + usize::from(emit_gap) + usize::from(immediate_stopped))?;
        self.check_changes(usize::from(emit_gap))?;
        if before {
            self.last_gap += 1;
            self.gaps.push(Self::gap(
                self.last_gap,
                GapReason::BeforeStart,
                None,
                self.definition.baseline_end,
                Some(at),
            ));
        }
        let mut out = Vec::new();
        if !already_open {
            self.last_gap += 1;
            let gap = Self::gap(self.last_gap, GapReason::Stopped, None, Some(at), None);
            self.gaps.push(gap.clone());
            if emit_gap {
                self.sequence += 1;
                out.push(self.batch(self.sequence, self.generation, vec![gap], at));
                self.change_sequence += 1;
            }
        }
        let previous = self.state;
        out.push(self.status(State::Stopping, Some("watch.stopped"), at));
        if previous == State::Lost && self.handle.is_none() {
            out.push(self.status(State::Stopped, Some("watch.stopped"), at));
        } else if let Some(handle) = self.handle {
            self.remove_pending = true;
            out.push(Action::Remove {
                token: self.token,
                generation: self.attempt_generation,
                handle,
            });
        }
        Ok(out)
    }
    fn stop_active(&mut self, at: u64) -> Result<Vec<Action>, Error> {
        self.check_gaps(1)?;
        let buffered = self
            .stream
            .as_ref()
            .ok_or(Error::InvalidState)?
            .buffered_len();
        self.check_sequences(buffered.div_ceil(32) + 2)?;
        self.check_changes(buffered.div_ceil(32) + 1)?;
        let stream = self.stream.as_mut().ok_or(Error::InvalidState)?;
        let batches = stream.flush(self.generation, at)?;
        let gap_seq = stream.reserve_status_sequence()?;
        let status_seq = stream.reserve_status_sequence()?;
        let gap_change_seq = stream.reserve_change_sequence()?;
        let mut out: Vec<_> = batches.into_iter().map(Action::Changes).collect();
        let mut changed: Vec<_> = self
            .gaps
            .iter_mut()
            .filter(|gap| gap.reason == GapReason::PartialCapability && gap.end.is_none())
            .map(|gap| {
                gap.end = Some(at);
                gap.clone()
            })
            .collect();
        self.last_gap += 1;
        let stopped = Self::gap(self.last_gap, GapReason::Stopped, None, Some(at), None);
        self.gaps.push(stopped.clone());
        changed.push(stopped);
        self.change_sequence = gap_change_seq - 1;
        out.push(self.batch(gap_seq, self.generation, changed, at));
        self.change_sequence = gap_change_seq;
        self.sequence = status_seq - 1;
        out.push(self.status(State::Stopping, Some("watch.stopped"), at));
        self.stream = None;
        if let Some(handle) = self.handle {
            self.remove_pending = true;
            out.push(Action::Remove {
                token: self.token,
                generation: self.attempt_generation,
                handle,
            });
        }
        Ok(out)
    }
    pub fn removed(
        &mut self,
        token: u64,
        handle: u64,
        result: Completion,
        at: u64,
    ) -> Result<Vec<Action>, Error> {
        if token != self.token
            || self.state != State::Stopping
            || !self.remove_pending
            || self.handle != Some(handle)
        {
            return Ok(Vec::new());
        }
        self.check_time(at)?;
        if result == Completion::Failed {
            self.remove_failed = true;
            self.last_time = at;
            return Ok(Vec::new());
        }
        self.check_sequences(1)?;
        self.handle = None;
        self.remove_pending = false;
        self.remove_failed = false;
        Ok(vec![self.status(State::Stopped, Some("watch.stopped"), at)])
    }
}
