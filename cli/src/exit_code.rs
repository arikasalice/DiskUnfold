//! The exit code table of the command line (F39 FR9.HD1.cli.DD1): rows are read
//! from the top and the first that matches decides. Only the process entry
//! turns the code into the process exit.
use std::process::ExitCode;

use du_core_contracts::{ErrorCode, job::JobState};

/// A refused call or argument: its stable reason key and envelope code.
pub(crate) struct Failure {
    pub(crate) reason: String,
    pub(crate) code: ErrorCode,
}

impl Failure {
    pub(crate) fn internal() -> Self {
        Self {
            reason: "support.internal_error".to_owned(),
            code: ErrorCode::Internal,
        }
    }

    pub(crate) fn argument(reason: &str) -> Self {
        Self {
            reason: reason.to_owned(),
            code: ErrorCode::InvalidArgument,
        }
    }
}

/// Reason keys of a usage error (exit 2): the arguments or the scope they name
/// cannot be used, including a path out of the result source's scope (F39 Q1).
const USAGE: &[&str] = &[
    "argument.missing",
    "path.undecodable",
    "path.unresolvable",
    "path.out_of_scope",
    "scope.empty",
    "scope.target_unavailable",
];

/// How a command ended, for the table.
pub(crate) enum End<'a> {
    /// Writing the result to stdout failed.
    OutputClosed,
    /// A second interrupt stopped the wait.
    Interrupted,
    /// A call or an argument was refused before the job ended.
    Refused(&'a Failure),
    /// The job reached its end state.
    Finished(JobState),
}

/// The exit code: output cut 5, interrupted or cancelled 4, usage error 2,
/// other error or failed 1, partial 3, otherwise 0. BUSY is 1.
pub(crate) fn exit(end: End<'_>) -> ExitCode {
    ExitCode::from(match end {
        End::OutputClosed => 5,
        End::Interrupted | End::Finished(JobState::Cancelled) => 4,
        End::Refused(failure)
            if failure.code == ErrorCode::InvalidArgument
                || failure.code == ErrorCode::ScopeDenied
                || USAGE.contains(&failure.reason.as_str()) =>
        {
            2
        }
        End::Refused(_) | End::Finished(JobState::Failed) => 1,
        End::Finished(JobState::Partial) => 3,
        End::Finished(_) => 0,
    })
}

#[cfg(test)]
mod tests;
