//! F15 startup memories written by the GUI (#195): `app.startup_memory_set`.
//!
//! The command remembers one state key in a hidden setting with one settings
//! commit, as the screen would, and publishes `settings.changed`. It is not
//! allowed while the app is closing; losing a dismissal at quit is accepted.
use std::sync::Arc;

use du_core_contracts::{
    settings::{SettingsChanges, SettingsCommit, SettingsSource, SettingsStoreError},
    settings_events::{
        SettingsApplyTiming, SettingsChangeSource, SettingsChangedBody, SettingsChangedKey,
    },
    startup_ipc::{StartupMemoryKind, StartupMemorySetRequest, StartupMemorySetResponse},
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    desktop::DesktopEvents,
    error::EngineError,
};
use du_user_env::{
    settings::{Changed, SettingsError, SettingsService},
    startup_memory::{NOTICE_DISMISSED, REVIEW_SEEN, is_state_key, remembered},
};
use serde_json::json;

use crate::{
    shared_database::SharedSettings,
    startup_checks::{StartupAccessSession, StartupMemories},
};

use super::{StartupSettings, current_time_ms};

fn key(kind: StartupMemoryKind) -> &'static str {
    match kind {
        StartupMemoryKind::NoticeDismissed => NOTICE_DISMISSED,
        StartupMemoryKind::ReviewSeen => REVIEW_SEEN,
    }
}

impl StartupSettings {
    /// Loads the remembered states into `session` and registers the GUI write.
    pub(crate) fn register_startup_memory(
        self: Arc<Self>,
        builder: &mut EngineBuilder,
        events: Arc<DesktopEvents>,
        session: Arc<StartupAccessSession>,
    ) {
        if let Ok(service) = self.service.lock() {
            session.load_memories(StartupMemories {
                notice_dismissed: remembered(service.snapshot(), NOTICE_DISMISSED),
                review_seen: remembered(service.snapshot(), REVIEW_SEEN),
            });
        }
        builder.register(SetStartupMemory {
            settings: self,
            events,
            session,
        });
    }
}

struct SetStartupMemory {
    settings: Arc<StartupSettings>,
    events: Arc<DesktopEvents>,
    session: Arc<StartupAccessSession>,
}

impl UseCase for SetStartupMemory {
    const NAME: &'static str = "app.startup_memory_set";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = StartupMemorySetRequest;
    type Response = StartupMemorySetResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        if !is_state_key(&request.state_key) {
            return Err(EngineError::new("command.body_invalid", vec![]));
        }
        let key = key(request.kind);
        let mut service = self
            .settings
            .service
            .lock()
            .map_err(|_| EngineError::new("support.internal_error", vec![]))?;
        // A revision that moved between the read and the commit is read again once.
        let written = match write(&mut service, key, &request.state_key) {
            Err(SettingsError::StaleVersion) => write(&mut service, key, &request.state_key),
            other => other,
        }
        .map_err(settings_error)?;
        if let Some((previous, changed)) = written {
            self.events.publish_settings_changed(
                SettingsChangedBody {
                    previous_settings_revision: previous.to_string(),
                    settings_revision: changed.revision.to_string(),
                    changes: changed
                        .keys
                        .into_iter()
                        .map(|key| SettingsChangedKey {
                            key,
                            apply_timing: SettingsApplyTiming::Immediate,
                        })
                        .collect(),
                    source: SettingsChangeSource::Screen,
                },
                Some(caller.correlation_id.clone()),
            );
        }
        let stored = remembered(service.snapshot(), key);
        self.session.remember(request.kind, stored.clone());
        Ok(StartupMemorySetResponse {
            kind: request.kind,
            state_key: stored.unwrap_or_default(),
        })
    }
}

/// Stores `value` under `key` with one commit, or nothing when it is already
/// stored. The empty string removes the value, so the default applies again.
fn write(
    service: &mut SettingsService<SharedSettings>,
    key: &str,
    value: &str,
) -> Result<Option<(u64, Changed)>, SettingsError> {
    if remembered(service.snapshot(), key).unwrap_or_default() == value {
        return Ok(None);
    }
    let revision = service.snapshot().revision;
    let changes = SettingsChanges::from([(key.to_owned(), Some(json!(value)))]);
    let reviewed = service.review(revision, &changes)?;
    let changed = service.apply(
        revision,
        &changes,
        reviewed,
        SettingsCommit {
            source: SettingsSource::Screen,
            timestamp_ms: current_time_ms(),
        },
    )?;
    Ok(Some((revision, changed)))
}

fn settings_error(error: SettingsError) -> EngineError {
    match error {
        SettingsError::Store(SettingsStoreError::Busy) => EngineError::new("db.busy", vec![]),
        SettingsError::InvalidValue => EngineError::new("command.body_invalid", vec![]),
        _ => EngineError::new("support.internal_error", vec![]),
    }
}

#[cfg(test)]
mod tests;
