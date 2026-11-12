//! F15 startup-check table validation and evidence aggregation.
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use du_core_contracts::startup::{
    FullDiskAccessStatus, PrivacySettingsOpenOutcome, StartupCapabilitiesPort, WebViewAvailability,
};
use du_core_contracts::startup_ipc::{
    StartupAccessAction, StartupAccessRequest, StartupAccessResponse, StartupAccessState,
    StartupCheck, StartupCheckOutcome, StartupCheckResult, StartupGrade, StartupStatus,
    StartupStatusRequest,
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

/// Holds the current startup permission observation and its explicit OS actions.
///
/// The status is refreshed only by the startup check or an IF-20 request. Unknown
/// observations remain permissive; only a confirmed denial blocks analysis entry.
pub(super) struct StartupAccessSession {
    capabilities: Arc<dyn StartupCapabilitiesPort>,
    admission: Mutex<()>,
    access: RwLock<StartupAccessState>,
}

impl StartupAccessSession {
    /// Samples the current permission state without prompting or opening settings.
    pub(super) fn new(capabilities: Arc<dyn StartupCapabilitiesPort>) -> Self {
        let access = access_state(capabilities.full_disk_access_status());
        Self {
            capabilities,
            admission: Mutex::new(()),
            access: RwLock::new(access),
        }
    }

    /// Serializes IF-20 session updates with synchronous analysis admission.
    ///
    /// This is an app-host ordering guarantee. The operating system can still
    /// change permissions outside this process after an observation is sampled.
    pub(super) fn lock_analysis_admission(&self) -> MutexGuard<'_, ()> {
        match self.admission.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Returns the current session observation, treating a poisoned lock as unknown.
    pub(super) fn current(&self) -> StartupAccessState {
        self.access
            .read()
            .map_or(StartupAccessState::Unknown, |access| *access)
    }

    pub(super) fn refresh(&self) -> StartupAccessState {
        let current = access_state(self.capabilities.full_disk_access_status());
        match self.access.write() {
            Ok(mut access) => *access = current,
            Err(error) => *error.into_inner() = current,
        }
        current
    }

    fn open_privacy_settings(&self) -> (StartupAccessState, bool) {
        let outcome = self.capabilities.open_privacy_settings();
        let access = self.refresh();
        // IF-20's current boolean represents a confirmed screen opening. An OS
        // accepting the URL launch request does not establish that fact.
        (
            access,
            outcome == PrivacySettingsOpenOutcome::ConfirmedOpened,
        )
    }
}

fn access_state(status: FullDiskAccessStatus) -> StartupAccessState {
    match status {
        FullDiskAccessStatus::Granted => StartupAccessState::Confirmed,
        FullDiskAccessStatus::Denied => StartupAccessState::Denied,
        FullDiskAccessStatus::Unknown => StartupAccessState::Unknown,
        FullDiskAccessStatus::NotApplicable => StartupAccessState::NotApplicable,
    }
}

/// Checks whether the compiled webview runtime is known to be unavailable before startup.
///
/// Unknown is not failure evidence: startup proceeds and the status row remains unverifiable.
pub(super) fn webview_observation(
    capabilities: &dyn StartupCapabilitiesPort,
) -> Result<StartupCheckResult, std::io::Error> {
    let (result, reason_key) = match capabilities.webview_availability() {
        WebViewAvailability::Available => (StartupCheckOutcome::Pass, None),
        WebViewAvailability::Unknown => (StartupCheckOutcome::Unverifiable, None),
        WebViewAvailability::Unavailable => {
            return Err(std::io::Error::other("startup.webview_missing"));
        }
    };
    Ok(StartupCheckResult {
        check: StartupCheck::Webview,
        result,
        reason_key,
    })
}

/// Shows a technical startup failure using the native pre-window dialog when available.
///
/// Lifecycle calls this on the launcher thread before constructing the webview. The original
/// error is retained so the launcher can still report it if a desktop dialog is unavailable.
pub(super) fn report_pre_window_error<E>(capabilities: &dyn StartupCapabilitiesPort, error: E) -> E
where
    E: std::fmt::Display,
{
    let _ =
        capabilities.show_pre_window_error_dialog("DiskUnfold could not start", &error.to_string());
    error
}

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
pub(super) fn register(
    builder: &mut EngineBuilder,
    report: StartupStatus,
    access: Arc<StartupAccessSession>,
) {
    builder.register(StartupStatusQuery {
        report,
        access: access.clone(),
    });
    builder.register(StartupAccessQuery { access });
}

struct StartupStatusQuery {
    report: StartupStatus,
    access: Arc<StartupAccessSession>,
}

impl UseCase for StartupStatusQuery {
    const NAME: &'static str = "app.startup_status";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = StartupStatusRequest;
    type Response = StartupStatus;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        let mut report = self.report.clone();
        let access = self.access.current();
        report.access = access;
        report.analysis_allowed = access != StartupAccessState::Denied;
        if let Some(row) = report
            .checks
            .iter_mut()
            .find(|row| row.check == StartupCheck::FullDiskAccess)
        {
            *row = access_result(access);
        }
        Ok(report)
    }
}

struct StartupAccessQuery {
    access: Arc<StartupAccessSession>,
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
        let _admission = self.access.lock_analysis_admission();
        let (access, settings_opened) = match request.action {
            StartupAccessAction::OpenPrivacySettings => self.access.open_privacy_settings(),
            StartupAccessAction::Recheck => (self.access.refresh(), false),
        };
        Ok(StartupAccessResponse {
            access,
            analysis_allowed: access != StartupAccessState::Denied,
            settings_opened,
        })
    }
}
#[cfg(test)]
mod tests;
