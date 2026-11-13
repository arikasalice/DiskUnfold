use super::*;
use du_core_contracts::locale_ipc::LocaleProfileChanges;

pub(super) fn settings_changes(
    current: &LocaleProfile,
    requested: &LocaleProfileChanges,
    supported: &[LocaleSupportedLanguage],
) -> Result<(SettingsChanges, Vec<LocaleChangedField>), EngineError> {
    let mut changes = SettingsChanges::new();
    let mut fields = Vec::new();
    append_display_language(&mut changes, &mut fields, current, requested, supported)?;
    append_date_format(&mut changes, &mut fields, current, requested)?;
    append_number_format(&mut changes, &mut fields, current, requested)?;
    append_time_zone_policy(&mut changes, &mut fields, current, requested)?;
    append_motion(&mut changes, &mut fields, current, requested)?;
    append_app_scale(&mut changes, &mut fields, current, requested)?;
    Ok((changes, fields))
}

fn append_display_language(
    changes: &mut SettingsChanges,
    fields: &mut Vec<LocaleChangedField>,
    current: &LocaleProfile,
    requested: &LocaleProfileChanges,
    supported: &[LocaleSupportedLanguage],
) -> Result<(), EngineError> {
    let Some(language) = &requested.display_language else {
        return Ok(());
    };
    if !supported.iter().any(|item| &item.language == language) {
        return Err(EngineError::new("locale.unsupported", vec![]));
    }
    if current.display_language != *language
        || current.language_source != LocaleLanguageSource::Stored
    {
        changes.insert(
            DISPLAY_LANGUAGE_KEY.to_owned(),
            Some(serde_json::json!(language)),
        );
        fields.push(LocaleChangedField::DisplayLanguage);
    }
    Ok(())
}

fn append_date_format(
    changes: &mut SettingsChanges,
    fields: &mut Vec<LocaleChangedField>,
    current: &LocaleProfile,
    requested: &LocaleProfileChanges,
) -> Result<(), EngineError> {
    let Some(value) = &requested.date_format else {
        return Ok(());
    };
    let parsed = match value.as_str() {
        "lang_default" => LocaleDateFormat::LangDefault,
        "iso8601" => LocaleDateFormat::Iso8601,
        _ => return Err(EngineError::new("locale.format_unknown", vec![])),
    };
    if parsed != current.date_format {
        changes.insert(DATE_FORMAT_KEY.into(), Some(serde_json::json!(value)));
        fields.push(LocaleChangedField::DateFormat);
    }
    Ok(())
}

fn append_number_format(
    changes: &mut SettingsChanges,
    fields: &mut Vec<LocaleChangedField>,
    current: &LocaleProfile,
    requested: &LocaleProfileChanges,
) -> Result<(), EngineError> {
    let Some(value) = &requested.number_format else {
        return Ok(());
    };
    let parsed = match value.as_str() {
        "lang_default" => LocaleNumberFormat::LangDefault,
        "no_grouping" => LocaleNumberFormat::NoGrouping,
        _ => return Err(EngineError::new("locale.format_unknown", vec![])),
    };
    if parsed != current.number_format {
        changes.insert(NUMBER_FORMAT_KEY.into(), Some(serde_json::json!(value)));
        fields.push(LocaleChangedField::NumberFormat);
    }
    Ok(())
}

fn append_time_zone_policy(
    changes: &mut SettingsChanges,
    fields: &mut Vec<LocaleChangedField>,
    current: &LocaleProfile,
    requested: &LocaleProfileChanges,
) -> Result<(), EngineError> {
    let Some(value) = &requested.time_zone_policy else {
        return Ok(());
    };
    let parsed = match value.as_str() {
        "local" => LocaleTimeZonePolicy::Local,
        "utc" => LocaleTimeZonePolicy::Utc,
        _ => return Err(EngineError::new("locale.format_unknown", vec![])),
    };
    if parsed != current.time_zone_policy {
        changes.insert(TIME_ZONE_POLICY_KEY.into(), Some(serde_json::json!(value)));
        fields.push(LocaleChangedField::TimeZonePolicy);
    }
    Ok(())
}

fn append_motion(
    changes: &mut SettingsChanges,
    fields: &mut Vec<LocaleChangedField>,
    current: &LocaleProfile,
    requested: &LocaleProfileChanges,
) -> Result<(), EngineError> {
    let Some(value) = &requested.motion else {
        return Ok(());
    };
    let parsed = match value.as_str() {
        "follow_os" => LocaleMotion::FollowOs,
        "reduce" => LocaleMotion::Reduce,
        _ => return Err(EngineError::new("settings.value_invalid", vec![])),
    };
    if parsed != current.motion {
        changes.insert(MOTION_KEY.into(), Some(serde_json::json!(value)));
        fields.push(LocaleChangedField::Motion);
    }
    Ok(())
}

fn append_app_scale(
    changes: &mut SettingsChanges,
    fields: &mut Vec<LocaleChangedField>,
    current: &LocaleProfile,
    requested: &LocaleProfileChanges,
) -> Result<(), EngineError> {
    let Some(value) = requested.app_scale else {
        return Ok(());
    };
    let parsed = LocaleAppScale::new(value)
        .ok_or_else(|| EngineError::new("settings.value_invalid", vec![]))?;
    if parsed != current.app_scale {
        changes.insert(APP_SCALE_KEY.into(), Some(serde_json::json!(value)));
        fields.push(LocaleChangedField::AppScale);
    }
    Ok(())
}
