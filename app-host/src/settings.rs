//! Compose stored Windows settings into the desktop scan registration.

use std::{io, sync::Arc};

use du_app_db::AppDb;
use du_core_contracts::settings::{SettingsStore, SharedTableLimit};
use du_engine_api::desktop::SharedTableLimitSource;
use du_user_env::space::restore_shared_table_limit;

/// Reads settings once at startup. Jobs retain a validated value at admission.
pub(crate) fn scan_limit_source() -> Result<SharedTableLimitSource, io::Error> {
    let data_dir = du_platform_os::application_data::directory()
        .ok_or_else(|| io::Error::other("db.open_failed: no Windows application data directory"))?;
    let mut database = AppDb::open_or_create_gui(&data_dir.join("diskunfold.db"))
        .map_err(|error| io::Error::other(format!("db.open_failed: {error:?}")))?;
    let limit = database
        .load()
        .ok()
        .and_then(|snapshot| restore_shared_table_limit(&snapshot))
        .unwrap_or_else(|| {
            eprintln!("settings.value_reset: space.shared_table_max_entries");
            SharedTableLimit::default()
        });
    Ok(Arc::new(move || limit))
}
