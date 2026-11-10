use super::*;

fn evidence() -> Reobservation {
    Reobservation {
        failed: false,
        processed: true,
        open_gap: false,
        last_gap_end: None,
        last_notification: None,
        started_at: 100,
        watching: true,
    }
}

#[test]
fn published_transitions_and_invalid_transitions_are_distinct() {
    use Event::*;
    use Settled::*;
    for (before, event, after) in [
        (Observed, Change, ChangeNotified),
        (Observed, Gap, GapUnknown),
        (ChangeNotified, Gap, GapUnknown),
        (Unwatched, Gap, GapUnknown),
        (Observed, WatchStopped, Unwatched),
    ] {
        assert_eq!(
            transition(State::Settled(before), event),
            Ok(State::Settled(after))
        );
    }
    assert_eq!(
        transition(State::Settled(GapUnknown), Change),
        Err(InvalidTransition)
    );
    assert_eq!(
        transition(State::Settled(Unwatched), WatchStopped),
        Err(InvalidTransition)
    );
    assert_eq!(
        transition(State::Settled(Observed), AbortRefresh),
        Err(InvalidTransition)
    );
}

#[test]
fn abort_and_unprocessed_restore_each_prior_state() {
    for prior in [
        Settled::Observed,
        Settled::Unwatched,
        Settled::ChangeNotified,
        Settled::GapUnknown,
        Settled::Unverifiable,
    ] {
        let pending = transition(State::Settled(prior), Event::BeginRefresh);
        assert_eq!(pending, Ok(State::Refreshing { previous: prior }));
        assert_eq!(
            transition(State::Refreshing { previous: prior }, Event::AbortRefresh),
            Ok(State::Settled(prior))
        );
        let input = Reobservation {
            processed: false,
            ..evidence()
        };
        assert_eq!(
            transition(
                State::Refreshing { previous: prior },
                Event::CompleteRefresh(input)
            ),
            Ok(State::Settled(prior))
        );
        assert_eq!(
            transition(State::Refreshing { previous: prior }, Event::BeginRefresh),
            Err(InvalidTransition)
        );
    }
}

#[test]
fn completion_uses_first_matching_rule_and_notification_boundary() {
    let prior = State::Refreshing {
        previous: Settled::Unwatched,
    };
    let cases = [
        (
            Reobservation {
                failed: true,
                processed: false,
                open_gap: true,
                last_notification: Some(100),
                ..evidence()
            },
            Settled::Unverifiable,
        ),
        (
            Reobservation {
                processed: false,
                open_gap: true,
                ..evidence()
            },
            Settled::Unwatched,
        ),
        (
            Reobservation {
                open_gap: true,
                last_notification: Some(100),
                ..evidence()
            },
            Settled::GapUnknown,
        ),
        (
            Reobservation {
                last_notification: Some(100),
                ..evidence()
            },
            Settled::ChangeNotified,
        ),
        (
            Reobservation {
                last_notification: Some(101),
                ..evidence()
            },
            Settled::ChangeNotified,
        ),
        (
            Reobservation {
                last_notification: Some(99),
                ..evidence()
            },
            Settled::Observed,
        ),
        (
            Reobservation {
                watching: false,
                ..evidence()
            },
            Settled::Unwatched,
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(
            transition(prior, Event::CompleteRefresh(input)),
            Ok(State::Settled(expected))
        );
    }
}

#[test]
fn a_closed_gap_requires_an_observation_started_after_its_end() {
    let refreshing = State::Refreshing {
        previous: Settled::GapUnknown,
    };
    for (end, watching, expected) in [
        (101, true, Settled::GapUnknown),
        (100, false, Settled::GapUnknown),
        (99, true, Settled::Observed),
        (99, false, Settled::Unwatched),
    ] {
        let input = Reobservation {
            last_gap_end: Some(end),
            watching,
            ..evidence()
        };
        assert_eq!(
            transition(refreshing, Event::CompleteRefresh(input)),
            Ok(State::Settled(expected))
        );
    }
}

#[test]
fn summary_preserves_the_published_order() {
    let states = [
        State::Settled(Settled::Observed),
        State::Settled(Settled::Unwatched),
        State::Refreshing {
            previous: Settled::Unverifiable,
        },
        State::Settled(Settled::ChangeNotified),
        State::Settled(Settled::GapUnknown),
        State::Settled(Settled::Unverifiable),
    ];
    let expected = [
        Summary::Observed,
        Summary::Unwatched,
        Summary::Refreshing,
        Summary::ChangeNotified,
        Summary::GapUnknown,
        Summary::Unverifiable,
    ];
    assert_eq!(worst([]), None);
    for (index, expected) in expected.into_iter().enumerate() {
        assert_eq!(worst(states[..=index].iter().copied()), Some(expected));
    }
}
