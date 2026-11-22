//! Restore the GUI database, settings and display language before the first window.
use std::{io, sync::Arc};

use du_core_contracts::{startup::StartupCapabilitiesPort, startup_ipc::StartupCheckResult};

use crate::{
    locale::StartupLocale, settings::StartupSettings, shared_database::SharedDatabase,
    startup_checks::report_pre_window_error,
};

/// Startup resources retained by the desktop assembly for the window lifetime.
pub(super) struct StartupState {
    pub(super) locale: Arc<StartupLocale>,
    pub(super) settings: Arc<StartupSettings>,
    pub(super) database: SharedDatabase,
    pub(super) first_run: bool,
    /// Data folder used by read-only history adapters.
    pub(super) data_dir: std::path::PathBuf,
}

/// Opens the GUI database under the instance lock, then restores settings and locale.
///
/// Appends the data format observation once the database is open. Every failure is
/// shown through the native pre-window dialog and returned so startup stops.
pub(super) fn open(
    capabilities: &dyn StartupCapabilitiesPort,
    startup_observed: &mut Vec<StartupCheckResult>,
) -> Result<StartupState, io::Error> {
    let report = |error: io::Error| report_pre_window_error(capabilities, error);
    let bundles = crate::locale::register().map_err(report)?;
    let configured = bundles.configured_languages().collect::<Vec<_>>();
    let data_dir = du_platform_os::application_data::directory()
        .ok_or_else(|| crate::startup_database::data_folder_error(None))
        .map_err(report)?;
    let opened = crate::startup_database::open_with_status(&data_dir).map_err(report)?;
    let first_run = opened.first_run;
    startup_observed.push(crate::startup_checks::data_format_and_open_observation());
    let database = opened.database;
    let initial_ruleset =
        du_engine_api::plan::built_in_protection_ruleset_row(crate::settings::current_time_ms())
            .map_err(|_| report(io::Error::other("protect.ruleset_unavailable")))?;
    database
        .initialize_builtin_ruleset_if_empty(&initial_ruleset)
        .map_err(|_| report(io::Error::other("protect.ruleset_unavailable")))?;
    let settings = StartupSettings::load(&database, &configured).map_err(report)?;
    let stored = settings.stored_language().map_err(report)?;
    let locale = Arc::new(crate::locale::initialize(bundles, stored.as_deref()).map_err(report)?);
    Ok(StartupState {
        locale,
        settings,
        database,
        first_run,
        data_dir,
    })
}
