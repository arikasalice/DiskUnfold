use super::*;

fn failure(reason: &str, code: ErrorCode) -> Failure {
    Failure {
        reason: reason.to_owned(),
        code,
    }
}

#[test]
fn each_end_takes_the_first_row_that_matches() {
    assert_eq!(exit(End::OutputClosed), ExitCode::from(5));
    assert_eq!(exit(End::Interrupted), ExitCode::from(4));
    assert_eq!(exit(End::Finished(JobState::Cancelled)), ExitCode::from(4));
    assert_eq!(exit(End::Finished(JobState::Failed)), ExitCode::from(1));
    assert_eq!(exit(End::Finished(JobState::Partial)), ExitCode::from(3));
    assert_eq!(exit(End::Finished(JobState::Done)), ExitCode::from(0));
}

#[test]
fn usage_errors_are_two_and_other_refusals_one() {
    for (reason, code) in [
        ("path.undecodable", ErrorCode::InvalidArgument),
        ("path.out_of_scope", ErrorCode::ScopeDenied),
        ("scope.target_unavailable", ErrorCode::Internal),
    ] {
        assert_eq!(
            exit(End::Refused(&failure(reason, code))),
            ExitCode::from(2),
            "{reason}"
        );
    }
    for (reason, code) in [
        ("db.busy", ErrorCode::Busy),
        ("scope.changed", ErrorCode::StaleVersion),
        ("support.internal_error", ErrorCode::Internal),
    ] {
        assert_eq!(
            exit(End::Refused(&failure(reason, code))),
            ExitCode::from(1),
            "{reason}"
        );
    }
}
