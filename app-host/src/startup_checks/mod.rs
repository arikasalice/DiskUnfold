//! F15 startup-check table validation and evidence aggregation.
use du_core_contracts::startup_ipc::{
    StartupAccessRequest, StartupAccessResponse, StartupAccessState, StartupCheck,
    StartupCheckOutcome, StartupCheckResult, StartupGrade, StartupStatus, StartupStatusRequest,
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

/// The fixed startup check registration row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Registration {
    /// Check identifier and fixed table position.
    pub check: StartupCheck,
    /// Whether a failure is allowed to stop startup or analysis entry.
    pub blocking: bool,
    /// Whether the check depends on network access.
    pub network_dependent: bool,
}

/// The checked-in F15 table follows the published DLD row order.
pub const REGISTRATIONS: [Registration; 7] = [
    Registration {
        check: StartupCheck::ResourceIntegrity,
        blocking: true,
        network_dependent: false,
    },
    Registration {
        check: StartupCheck::Webview,
        blocking: true,
        network_dependent: false,
    },
    Registration {
        check: StartupCheck::DataFormatAndOpen,
        blocking: true,
        network_dependent: false,
    },
    Registration {
        check: StartupCheck::TemporaryFiles,
        blocking: false,
        network_dependent: false,
    },
    Registration {
        check: StartupCheck::SupportMatrix,
        blocking: false,
        network_dependent: false,
    },
    Registration {
        check: StartupCheck::Capability,
        blocking: false,
        network_dependent: false,
    },
    Registration {
        check: StartupCheck::FullDiskAccess,
        blocking: true,
        network_dependent: false,
    },
];

/// Invalid startup check table; no partially validated table is accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationError {
    /// A required check is missing or the table has extra rows.
    MissingOrExtra,
    /// A check appears more than once.
    Duplicate,
    /// Checks are not in the fixed published order.
    Order,
    /// A check requires network access.
    NetworkDependency,
    /// The check's blocking policy differs from the published policy.
    BlockingPolicy,
}

/// A startup report cannot be published when its observations contradict the table policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusError {
    /// An observation names a check outside the fixed table.
    UnknownObservation,
    /// The same startup check was observed more than once.
    DuplicateObservation,
    /// A technical prerequisite failed before the application window opened.
    RequiredCheckBlocked(StartupCheck),
    /// An advisory check reported a blocking outcome, which the table forbids.
    AdvisoryCheckBlocked(StartupCheck),
    /// An explicit full-disk-access row conflicts with the access state.
    AccessObservationMismatch,
}

/// Validates the fixed seven-row order, blocking policy, and offline requirement.
pub fn validate(registrations: &[Registration]) -> Result<(), RegistrationError> {
    if registrations.len() != REGISTRATIONS.len() {
        return Err(RegistrationError::MissingOrExtra);
    }

    for (index, row) in registrations.iter().enumerate() {
        if registrations[..index]
            .iter()
            .any(|earlier| earlier.check == row.check)
        {
            return Err(RegistrationError::Duplicate);
        }
        if row.network_dependent {
            return Err(RegistrationError::NetworkDependency);
        }
        let expected = REGISTRATIONS[index];
        if row.check != expected.check {
            return Err(RegistrationError::Order);
        }
        if row.blocking != expected.blocking {
            return Err(RegistrationError::BlockingPolicy);
        }
    }
    Ok(())
}

/// Fills every fixed row, leaving unobserved rows explicitly unverifiable.
pub fn status(
    first_run: bool,
    access: du_core_contracts::startup_ipc::StartupAccessState,
    observed: &[StartupCheckResult],
) -> Result<StartupStatus, StatusError> {
    for (index, result) in observed.iter().enumerate() {
        if result.check == StartupCheck::Unknown
            || !REGISTRATIONS
                .iter()
                .any(|registration| registration.check == result.check)
        {
            return Err(StatusError::UnknownObservation);
        }
        if observed[..index]
            .iter()
            .any(|earlier| earlier.check == result.check)
        {
            return Err(StatusError::DuplicateObservation);
        }
        if result.result == StartupCheckOutcome::Blocked {
            if result.check == StartupCheck::FullDiskAccess {
                if access != StartupAccessState::Denied {
                    return Err(StatusError::AccessObservationMismatch);
                }
            } else if REGISTRATIONS
                .iter()
                .find(|registration| registration.check == result.check)
                .is_some_and(|registration| registration.blocking)
            {
                return Err(StatusError::RequiredCheckBlocked(result.check));
            } else {
                return Err(StatusError::AdvisoryCheckBlocked(result.check));
            }
        }
    }

    let checks = REGISTRATIONS
        .iter()
        .map(|registration| {
            if registration.check == StartupCheck::FullDiskAccess {
                return access_result(access);
            }
            observed
                .iter()
                .find(|result| result.check == registration.check)
                .cloned()
                .unwrap_or(StartupCheckResult {
                    check: registration.check,
                    result: StartupCheckOutcome::Unverifiable,
                    reason_key: None,
                })
        })
        .collect::<Vec<_>>();
    let grade = grade(&checks).unwrap_or(StartupGrade::Unverifiable);
    Ok(StartupStatus {
        grade,
        checks,
        first_run,
        access,
        analysis_allowed: access != StartupAccessState::Denied,
    })
}

/// Maps the database startup observation without inferring directory writability from an open.
///
/// A successfully published first database proves the data-directory write path. Opening an
/// existing database verifies its format and connection only, so that combined row stays unknown.
pub(super) fn data_format_and_open_observation(first_run: bool) -> StartupCheckResult {
    StartupCheckResult {
        check: StartupCheck::DataFormatAndOpen,
        result: if first_run {
            StartupCheckOutcome::Pass
        } else {
            StartupCheckOutcome::Unverifiable
        },
        reason_key: None,
    }
}

fn access_result(access: StartupAccessState) -> StartupCheckResult {
    let (result, reason_key) = match access {
        StartupAccessState::Confirmed => (StartupCheckOutcome::Pass, None),
        StartupAccessState::Denied => (
            StartupCheckOutcome::Blocked,
            Some("startup.full_disk_access_required".to_owned()),
        ),
        StartupAccessState::Unknown => (StartupCheckOutcome::Unverifiable, None),
        StartupAccessState::NotApplicable => (StartupCheckOutcome::NotApplicable, None),
    };
    StartupCheckResult {
        check: StartupCheck::FullDiskAccess,
        result,
        reason_key,
    }
}

/// Combines applicable outcomes without letting access denial change the grade.
pub fn grade(checks: &[StartupCheckResult]) -> Option<StartupGrade> {
    checks
        .iter()
        .filter(|row| row.check != StartupCheck::FullDiskAccess)
        .filter_map(|row| match row.result {
            StartupCheckOutcome::Pass | StartupCheckOutcome::NotApplicable => None,
            StartupCheckOutcome::Limited => Some(StartupGrade::Limited),
            StartupCheckOutcome::OutsideVerified => Some(StartupGrade::OutsideVerified),
            StartupCheckOutcome::Unverifiable | StartupCheckOutcome::Unknown => {
                Some(StartupGrade::Unverifiable)
            }
            StartupCheckOutcome::Blocked => None,
        })
        .max_by_key(|grade| match grade {
            StartupGrade::Pass => 0,
            StartupGrade::Limited => 1,
            StartupGrade::OutsideVerified => 2,
            StartupGrade::Unverifiable | StartupGrade::Unknown => 3,
        })
        .or_else(|| {
            checks
                .iter()
                .any(|row| {
                    row.check != StartupCheck::FullDiskAccess
                        && row.result == StartupCheckOutcome::Pass
                })
                .then_some(StartupGrade::Pass)
        })
}

/// Registers the app-host startup queries with the composed GUI engine.
pub(super) fn register(builder: &mut EngineBuilder, report: StartupStatus) {
    builder.register(StartupStatusQuery(report.clone()));
    builder.register(StartupAccessQuery {
        access: report.access,
    });
}

/// Registers IF-20 independently when the platform startup database path is absent.
#[cfg(target_os = "macos")]
pub(super) fn register_access(builder: &mut EngineBuilder) {
    builder.register(StartupAccessQuery {
        access: StartupAccessState::Unknown,
    });
}

struct StartupStatusQuery(StartupStatus);

impl UseCase for StartupStatusQuery {
    const NAME: &'static str = "app.startup_status";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = StartupStatusRequest;
    type Response = StartupStatus;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        Ok(self.0.clone())
    }
}

struct StartupAccessQuery {
    access: StartupAccessState,
}

impl UseCase for StartupAccessQuery {
    const NAME: &'static str = "app.startup_access";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = StartupAccessRequest;
    type Response = StartupAccessResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        // The platform-os startup observation port is not present in this base.
        // Unknown access does not deny analysis and settings are never reported as opened.
        let _requested_action = request.action;
        Ok(StartupAccessResponse {
            access: self.access,
            analysis_allowed: self.access != StartupAccessState::Denied,
            settings_opened: false,
        })
    }
}
#[cfg(test)]
mod tests;
