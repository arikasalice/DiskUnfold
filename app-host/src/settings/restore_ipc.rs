//! Publish only the immutable, value-free settings restoration report to the GUI.
use du_core_contracts::settings_ipc::{
    SettingsRestoreItem, SettingsRestoreRequest, SettingsRestoreSource, SettingsRestoreStatus,
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};
use du_user_env::settings::RestoreSource;

use super::StartupSettings;

impl StartupSettings {
    /// Registers the report independently of the mutable settings service.
    pub(crate) fn register_restore_status(&self, builder: &mut EngineBuilder) {
        let status = &self.status;
        builder.register(RestoreStatusQuery(SettingsRestoreStatus {
            items: status
                .items
                .iter()
                .map(|(key, item)| SettingsRestoreItem {
                    key: key.clone(),
                    source: match item.source {
                        RestoreSource::Stored => SettingsRestoreSource::Restored,
                        RestoreSource::Default => SettingsRestoreSource::Default,
                    },
                    reason_key: item.reason.map(str::to_owned),
                })
                .collect(),
            stored_schema: status.stored_schema,
            app_schema: status.app_schema,
            migrated: status.migrated,
            write_locked: status.write_locked,
        }));
    }
}

struct RestoreStatusQuery(SettingsRestoreStatus);

impl UseCase for RestoreStatusQuery {
    const NAME: &'static str = "settings.restore_status";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SettingsRestoreRequest;
    type Response = SettingsRestoreStatus;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        Ok(self.0.clone())
    }
}
