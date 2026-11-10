//! F33 freshness decisions for one range, independent of storage and events.
//!
//! This module does not publish an IF-17 ledger or attach state to a result.
//! Its values are local implementation types pending the shared contracts.

/// A non-refreshing freshness state in the published severity order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Settled {
    Observed,
    Unwatched,
    ChangeNotified,
    GapUnknown,
    Unverifiable,
}

/// One range, including its pre-refresh state for cancellation and failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Settled(Settled),
    Refreshing { previous: Settled },
}

/// Evidence for a completed observation of a refresh range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reobservation {
    /// The range could not be observed because of an access or target failure.
    pub failed: bool,
    /// The range was included in the completed observation.
    pub processed: bool,
    /// An observation gap remains open for the range.
    pub open_gap: bool,
    /// End of the most recent closed gap, in the same clock as `started_at`.
    pub last_gap_end: Option<u64>,
    /// Last change notification for the range, in the same clock as `started_at`.
    pub last_notification: Option<u64>,
    pub started_at: u64,
    pub watching: bool,
}

/// Events in the published freshness state diagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Change,
    Gap,
    WatchStopped,
    BeginRefresh,
    /// A cancelled, failed or superseded refresh restores the prior state.
    AbortRefresh,
    /// A published result supplies the completed range's observation evidence.
    CompleteRefresh(Reobservation),
}

/// No transition exists for this event and state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTransition;

/// Applies one published state transition; an invalid transition changes nothing.
pub fn transition(state: State, event: Event) -> Result<State, InvalidTransition> {
    use Event::{AbortRefresh, BeginRefresh, Change, CompleteRefresh, Gap, WatchStopped};
    use Settled::{ChangeNotified, GapUnknown, Observed, Unwatched};
    match (state, event) {
        (State::Settled(Observed), Change) => Ok(State::Settled(ChangeNotified)),
        (State::Settled(Observed | ChangeNotified | Unwatched), Gap) => {
            Ok(State::Settled(GapUnknown))
        }
        (State::Settled(Observed), WatchStopped) => Ok(State::Settled(Unwatched)),
        (State::Settled(previous), BeginRefresh) => Ok(State::Refreshing { previous }),
        (State::Refreshing { previous }, AbortRefresh) => Ok(State::Settled(previous)),
        (State::Refreshing { previous }, CompleteRefresh(evidence)) => {
            Ok(State::Settled(reobserved(previous, evidence)))
        }
        _ => Err(InvalidTransition),
    }
}

fn reobserved(previous: Settled, evidence: Reobservation) -> Settled {
    if evidence.failed {
        return Settled::Unverifiable;
    }
    if !evidence.processed {
        return previous;
    }
    if evidence.open_gap
        || evidence
            .last_gap_end
            .is_some_and(|end| end >= evidence.started_at)
    {
        return Settled::GapUnknown;
    }
    if evidence
        .last_notification
        .is_some_and(|last| last >= evidence.started_at)
    {
        return Settled::ChangeNotified;
    }
    if evidence.watching {
        Settled::Observed
    } else {
        Settled::Unwatched
    }
}

/// Representative state without a range-specific pre-refresh value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Summary {
    Observed,
    Unwatched,
    Refreshing,
    ChangeNotified,
    GapUnknown,
    Unverifiable,
}

/// Returns the worst state for a nonempty set in published severity order.
pub fn worst(states: impl IntoIterator<Item = State>) -> Option<Summary> {
    states
        .into_iter()
        .map(|state| match state {
            State::Settled(Settled::Observed) => Summary::Observed,
            State::Settled(Settled::Unwatched) => Summary::Unwatched,
            State::Refreshing { .. } => Summary::Refreshing,
            State::Settled(Settled::ChangeNotified) => Summary::ChangeNotified,
            State::Settled(Settled::GapUnknown) => Summary::GapUnknown,
            State::Settled(Settled::Unverifiable) => Summary::Unverifiable,
        })
        .max()
}

#[cfg(test)]
mod tests;
