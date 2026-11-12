//! GUI-only locale profile reads and compare-and-set updates.

use std::sync::Arc;

use du_core_contracts::{
    Timestamp,
    locale_ipc::{
        LocalTimeZoneStatus, LocaleAppScale, LocaleChangedBody, LocaleChangedField,
        LocaleDateFormat, LocaleLanguageSource, LocaleMotion, LocaleNumberFormat, LocaleProfile,
        LocaleProfileRevision, LocaleProfileSetRequest, LocaleSupportedLanguage,
        LocaleTimeZonePolicy,
    },
    settings::{SettingsChanges, SettingsCommit, SettingsSource, SettingsStoreError},
    settings_events::{
        SettingsApplyTiming, SettingsChangeSource, SettingsChangedBody, SettingsChangedKey,
    },
    time_zone::{TimeZonePort, TimeZoneUnavailable},
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    desktop::DesktopEvents,
    error::EngineError,
};
use du_user_env::{
    CatalogEntry,
    locale::DISPLAY_LANGUAGE_KEY,
    settings::{SettingsError, SettingsService},
};
use serde_json::Value;

use crate::{locale::StartupLocale, shared_database::SharedSettings};

use super::{StartupSettings, current_time_ms};

mod changes;
use changes::settings_changes;

const DATE_FORMAT_KEY: &str = "locale.date_format";
const NUMBER_FORMAT_KEY: &str = "locale.number_format";
const TIME_ZONE_POLICY_KEY: &str = "locale.time_zone_policy";
const MOTION_KEY: &str = "a11y.motion";
const APP_SCALE_KEY: &str = "a11y.app_scale";

/// Supplies the real, integrity-checked bundle inventory to the host profile.
///
/// Revisions must come from user-env's admitted bundle metadata. Callers must not
/// synthesize revisions or include bundles rejected during startup validation.
pub(crate) type SupportedLanguageInventory =
    Arc<dyn Fn() -> Result<Vec<LocaleSupportedLanguage>, &'static str> + Send + Sync>;

/// Dependencies needed to serve the desktop locale profile and its events.
pub(super) struct LocaleRuntime {
    startup_locale: Arc<StartupLocale>,
    events: Arc<DesktopEvents>,
    supported_languages: SupportedLanguageInventory,
    time_zone: Arc<dyn TimeZonePort>,
}

impl LocaleRuntime {
    /// Binds startup language, verified bundle inventory, event delivery, and OS time-zone evidence.
    pub(crate) fn new(
        startup_locale: Arc<StartupLocale>,
        events: Arc<DesktopEvents>,
        supported_languages: SupportedLanguageInventory,
        time_zone: Arc<dyn TimeZonePort>,
    ) -> Self {
        Self {
            startup_locale,
            events,
            supported_languages,
            time_zone,
        }
    }
}

struct LocaleProfileContext {
    settings: Arc<StartupSettings>,
    runtime: LocaleRuntime,
}

impl StartupSettings {
    /// Registers locale profile reads and writes for one GUI window context.
    pub(crate) fn register_locale_profile(
        self: Arc<Self>,
        builder: &mut EngineBuilder,
        runtime: LocaleRuntime,
        primary_window: bool,
    ) {
        let context = Arc::new(LocaleProfileContext {
            settings: self,
            runtime,
        });
        builder.register(GetLocaleProfile(context.clone()));
        builder.register(SetLocaleProfile {
            context,
            primary_window,
        });
    }
}

struct GetLocaleProfile(Arc<LocaleProfileContext>);

impl UseCase for GetLocaleProfile {
    const NAME: &'static str = "i18n.get_profile";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = du_core_contracts::locale_ipc::LocaleGetProfileRequest;
    type Response = LocaleProfile;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        let service = self
            .0
            .settings
            .service
            .lock()
            .map_err(|_| EngineError::new("support.internal_error", vec![]))?;
        current_profile(
            &service,
            &self.0.runtime.startup_locale,
            &self.0.runtime.supported_languages,
            self.0.runtime.time_zone.as_ref(),
        )
    }
}

struct SetLocaleProfile {
    context: Arc<LocaleProfileContext>,
    primary_window: bool,
}

impl UseCase for SetLocaleProfile {
    const NAME: &'static str = "i18n.set_profile";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = LocaleProfileSetRequest;
    type Response = LocaleProfile;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        if self.primary_window {
            Ok(())
        } else {
            Err(EngineError::new("command.window_denied", vec![]))
        }
    }

    fn execute(
        &self,
        request: Self::Request,
        caller: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let mut service = self
            .context
            .settings
            .service
            .lock()
            .map_err(|_| EngineError::new("support.internal_error", vec![]))?;
        let revision = service.snapshot().revision;
        if request.expected_profile_revision.as_u64() != revision {
            return Err(EngineError::new("locale.profile_modified", vec![]));
        }

        let supported = supported_languages(&self.context.runtime.supported_languages)?;
        let current = current_profile_from_inventory(
            &service,
            &self.context.runtime.startup_locale,
            &supported,
            self.context.runtime.time_zone.as_ref(),
        )?;
        let (changes, changed_fields) = settings_changes(&current, &request.changes, &supported)?;
        if changes.is_empty() {
            return Ok(current);
        }

        let reviewed = service.review(revision, &changes).map_err(settings_error)?;
        let committed = service
            .apply(
                revision,
                &changes,
                reviewed,
                SettingsCommit {
                    source: SettingsSource::Screen,
                    timestamp_ms: current_time_ms(),
                },
            )
            .map_err(settings_error)?;

        self.context.runtime.events.publish_settings_changed(
            SettingsChangedBody {
                previous_settings_revision: revision.to_string(),
                settings_revision: committed.revision.to_string(),
                changes: committed
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

        let updated = current_profile_from_inventory(
            &service,
            &self.context.runtime.startup_locale,
            &supported,
            self.context.runtime.time_zone.as_ref(),
        )?;
        self.context.runtime.events.publish_locale_changed(
            LocaleChangedBody {
                profile_revision: updated.profile_revision.clone(),
                changed_fields,
                display_language: updated.display_language.clone(),
            },
            Some(caller.correlation_id.clone()),
        );
        Ok(updated)
    }
}

fn supported_languages(
    inventory: &SupportedLanguageInventory,
) -> Result<Vec<LocaleSupportedLanguage>, EngineError> {
    let languages = inventory().map_err(|reason| EngineError::new(reason, vec![]))?;
    if !languages.iter().any(|item| item.language == "en")
        || languages.iter().any(|item| {
            item.language.is_empty()
                || item.self_name_key.is_empty()
                || item.bundle_revision.is_empty()
        })
        || languages
            .windows(2)
            .any(|pair| pair[0].language >= pair[1].language)
    {
        return Err(EngineError::new("locale.registration_failed", vec![]));
    }
    Ok(languages)
}

fn current_profile(
    service: &SettingsService<SharedSettings>,
    startup_locale: &StartupLocale,
    inventory: &SupportedLanguageInventory,
    time_zone: &dyn TimeZonePort,
) -> Result<LocaleProfile, EngineError> {
    let supported = supported_languages(inventory)?;
    current_profile_from_inventory(service, startup_locale, &supported, time_zone)
}

fn current_profile_from_inventory(
    service: &SettingsService<SharedSettings>,
    startup_locale: &StartupLocale,
    supported: &[LocaleSupportedLanguage],
    time_zone: &dyn TimeZonePort,
) -> Result<LocaleProfile, EngineError> {
    let (language, language_source) = selected_language(service, startup_locale, supported)?;
    let revision = service.snapshot().revision;
    Ok(LocaleProfile {
        display_language: language,
        language_source,
        date_format: date_format(service)?,
        number_format: number_format(service)?,
        time_zone_policy: time_zone_policy(service)?,
        local_time_zone_status: local_time_zone_status(time_zone),
        motion: motion(service)?,
        app_scale: app_scale(service)?,
        profile_revision: LocaleProfileRevision::from_u64(revision),
        supported_languages: supported.to_vec(),
    })
}

fn date_format(service: &SettingsService<SharedSettings>) -> Result<LocaleDateFormat, EngineError> {
    match setting_str(service, DATE_FORMAT_KEY)? {
        "lang_default" => Ok(LocaleDateFormat::LangDefault),
        "iso8601" => Ok(LocaleDateFormat::Iso8601),
        _ => Err(EngineError::new("support.internal_error", vec![])),
    }
}

fn number_format(
    service: &SettingsService<SharedSettings>,
) -> Result<LocaleNumberFormat, EngineError> {
    match setting_str(service, NUMBER_FORMAT_KEY)? {
        "lang_default" => Ok(LocaleNumberFormat::LangDefault),
        "no_grouping" => Ok(LocaleNumberFormat::NoGrouping),
        _ => Err(EngineError::new("support.internal_error", vec![])),
    }
}

fn time_zone_policy(
    service: &SettingsService<SharedSettings>,
) -> Result<LocaleTimeZonePolicy, EngineError> {
    match setting_str(service, TIME_ZONE_POLICY_KEY)? {
        "local" => Ok(LocaleTimeZonePolicy::Local),
        "utc" => Ok(LocaleTimeZonePolicy::Utc),
        _ => Err(EngineError::new("support.internal_error", vec![])),
    }
}

fn motion(service: &SettingsService<SharedSettings>) -> Result<LocaleMotion, EngineError> {
    match setting_str(service, MOTION_KEY)? {
        "follow_os" => Ok(LocaleMotion::FollowOs),
        "reduce" => Ok(LocaleMotion::Reduce),
        _ => Err(EngineError::new("support.internal_error", vec![])),
    }
}

fn app_scale(service: &SettingsService<SharedSettings>) -> Result<LocaleAppScale, EngineError> {
    setting_value(service, APP_SCALE_KEY)?
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .and_then(LocaleAppScale::new)
        .ok_or_else(|| EngineError::new("support.internal_error", vec![]))
}

fn selected_language(
    service: &SettingsService<SharedSettings>,
    startup_locale: &StartupLocale,
    supported: &[LocaleSupportedLanguage],
) -> Result<(String, LocaleLanguageSource), EngineError> {
    if let Some(language) = service
        .snapshot()
        .values
        .get(DISPLAY_LANGUAGE_KEY)
        .and_then(Value::as_str)
        && supported.iter().any(|item| item.language == language)
    {
        return Ok((language.to_owned(), LocaleLanguageSource::Stored));
    }
    let language = startup_locale.language();
    if !supported.iter().any(|item| item.language == language) {
        return Err(EngineError::new("locale.registration_failed", vec![]));
    }
    let source = startup_locale
        .source()
        .ok_or_else(|| EngineError::new("support.internal_error", vec![]))?;
    Ok((language.to_owned(), source))
}

fn setting_value<'a>(
    service: &'a SettingsService<SharedSettings>,
    key: &str,
) -> Result<&'a Value, EngineError> {
    service
        .catalog()
        .1
        .into_iter()
        .find(|entry: &CatalogEntry<'_>| entry.descriptor.key == key)
        .map(|entry| entry.value)
        .ok_or_else(|| EngineError::new("support.internal_error", vec![]))
}

fn setting_str<'a>(
    service: &'a SettingsService<SharedSettings>,
    key: &str,
) -> Result<&'a str, EngineError> {
    setting_value(service, key)?
        .as_str()
        .ok_or_else(|| EngineError::new("support.internal_error", vec![]))
}

fn local_time_zone_status(time_zone: &dyn TimeZonePort) -> LocalTimeZoneStatus {
    let Some(instant) = Timestamp::from_unix_millis(current_time_ms()) else {
        return LocalTimeZoneStatus::Unavailable;
    };
    match time_zone.observe(&instant) {
        Ok(observation) if observation.identity.is_some() => LocalTimeZoneStatus::Identified,
        Ok(_) => LocalTimeZoneStatus::OffsetOnly,
        Err(TimeZoneUnavailable) => LocalTimeZoneStatus::Unavailable,
    }
}

fn settings_error(error: SettingsError) -> EngineError {
    match error {
        SettingsError::StaleVersion => EngineError::new("locale.profile_modified", vec![]),
        SettingsError::Store(SettingsStoreError::Conflict) => {
            EngineError::new("locale.profile_modified", vec![])
        }
        SettingsError::Store(SettingsStoreError::Busy) => EngineError::new("db.busy", vec![]),
        SettingsError::InvalidValue => EngineError::new("settings.value_invalid", vec![]),
        SettingsError::UnknownKey
        | SettingsError::NotEditable
        | SettingsError::StaleReview
        | SettingsError::Store(SettingsStoreError::Storage)
        | SettingsError::Encoding
        | SettingsError::SchemaNewer
        | SettingsError::SchemaUnsupported
        | SettingsError::InvalidStoredValue
        | SettingsError::RestoreLocked => EngineError::new("support.internal_error", vec![]),
    }
}

#[cfg(test)]
mod tests;
