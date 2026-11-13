//! GUI-only read of effective F03 display preferences.
use std::sync::Arc;

use du_core_contracts::{
    SizeBasis,
    settings_ipc::{DisplayPreferences, DisplayPreferencesRequest, DisplayUnitSystem},
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};
use du_user_env::space::{DEFAULT_BASIS, SIZE_UNIT, SIZE_UNIT_SYSTEM};

use super::StartupSettings;

impl StartupSettings {
    /// Registers a read-only query retaining the live settings service.
    pub(crate) fn register_display_preferences(self: Arc<Self>, builder: &mut EngineBuilder) {
        builder.register(DisplayPreferencesQuery(self));
    }
}

struct DisplayPreferencesQuery(Arc<StartupSettings>);

impl UseCase for DisplayPreferencesQuery {
    const NAME: &'static str = "space.display_preferences";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = DisplayPreferencesRequest;
    type Response = DisplayPreferences;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        let service = self
            .0
            .service
            .lock()
            .map_err(|_| EngineError::new("settings.restore_failed", vec![]))?;
        let units = service
            .display_units()
            .ok_or_else(|| EngineError::new("settings.restore_failed", vec![]))?;
        let system = match units.system {
            "si" => DisplayUnitSystem::Si,
            "iec" => DisplayUnitSystem::Iec,
            _ => return Err(EngineError::new("settings.restore_failed", vec![])),
        };
        let (revision, catalog) = service.catalog();
        let basis = catalog
            .iter()
            .find(|row| row.descriptor.key == DEFAULT_BASIS)
            .and_then(|row| row.value.as_str())
            .and_then(|basis| match basis {
                "logical" => Some(SizeBasis::Logical),
                "allocated" => Some(SizeBasis::Allocated),
                "unique_physical" => Some(SizeBasis::UniquePhysical),
                _ => None,
            })
            .ok_or_else(|| EngineError::new("settings.restore_failed", vec![]))?;
        let status = service.restore_status();
        let warning = status
            .failure_reason
            .or_else(|| {
                [SIZE_UNIT_SYSTEM, SIZE_UNIT, DEFAULT_BASIS]
                    .iter()
                    .find_map(|key| {
                        status
                            .items
                            .get(*key)
                            .and_then(|item| item.reason)
                            .filter(|reason| {
                                !(*key == SIZE_UNIT
                                    && *reason == "settings.value_reset"
                                    && units.warning.is_none())
                            })
                    })
            })
            .or(units.warning);
        Ok(DisplayPreferences {
            settings_revision: revision.to_string(),
            unit_system: system,
            unit: units.unit.to_owned(),
            default_basis: basis,
            warning_key: warning.map(str::to_owned),
        })
    }
}
