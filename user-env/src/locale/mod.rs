//! Pure startup language selection and F13 setting descriptors.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::{Descriptor, ValueKind};

/// Settings key for the user-selected display language.
pub const DISPLAY_LANGUAGE_KEY: &str = "locale.display_language";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsLanguage<'a> {
    Supported(&'a [&'a str]),
    Partial(&'a [&'a str]),
    /// Includes unsupported, failed and unrecognized capability values.
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Argument,
    Stored,
    Os,
    Baseline,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Decision {
    pub language: String,
    pub source: Source,
    /// The stored value was considered but could not be used.
    pub stored_unavailable: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DecisionError {
    UnsupportedLanguage,
    RegistrationFailed,
}

impl DecisionError {
    pub const fn reason_key(&self) -> &'static str {
        match self {
            Self::UnsupportedLanguage => "locale.unsupported",
            Self::RegistrationFailed => "locale.registration_failed",
        }
    }
}

/// All inputs are observations; the decision never mutates persisted settings.
pub struct Inputs<'a> {
    pub registered: &'a [&'a str],
    pub stored: Option<&'a str>,
    pub os: OsLanguage<'a>,
    pub korean_integrity_failed: bool,
}

impl Inputs<'_> {
    fn contains(&self, language: &str) -> bool {
        self.registered.contains(&language)
    }

    fn decision(&self, language: &str, source: Source, stored_unavailable: bool) -> Decision {
        Decision {
            language: language.to_owned(),
            source,
            stored_unavailable,
        }
    }

    fn valid_baseline(&self) -> Result<(), DecisionError> {
        if self.contains("en") {
            Ok(())
        } else {
            Err(DecisionError::RegistrationFailed)
        }
    }
}

/// OS tags use only the first preference and the primary subtag.
pub fn primary_language(tag: &str) -> Option<String> {
    let primary = tag.split(['-', '_']).next()?;
    if primary.is_empty() {
        None
    } else {
        Some(primary.to_ascii_lowercase())
    }
}

fn os_choice(inputs: &Inputs<'_>) -> Option<String> {
    let tags = match inputs.os {
        OsLanguage::Supported(tags) | OsLanguage::Partial(tags) => tags,
        OsLanguage::Unavailable => return None,
    };
    let language = primary_language(tags.first()?)?;
    inputs.contains(&language).then_some(language)
}

fn stored_choice(inputs: &Inputs<'_>) -> Option<(String, Source)> {
    inputs
        .stored
        .filter(|language| inputs.contains(language))
        .map(|language| (language.to_owned(), Source::Stored))
}

fn operating_system_choice(inputs: &Inputs<'_>) -> Option<(String, Source)> {
    os_choice(inputs).map(|language| (language, Source::Os))
}

type Rule = fn(&Inputs<'_>) -> Option<(String, Source)>;

fn choose(inputs: &Inputs<'_>) -> Decision {
    let invalid_stored = inputs
        .stored
        .is_some_and(|language| !inputs.contains(language));
    let rules: [Rule; 2] = [stored_choice, operating_system_choice];
    for rule in rules {
        if let Some((language, source)) = rule(inputs) {
            return inputs.decision(&language, source, invalid_stored);
        }
    }
    inputs.decision("en", Source::Baseline, invalid_stored)
}
/// Evaluate the GUI startup table after bundle registration.
pub fn decide(inputs: &Inputs<'_>) -> Result<Decision, DecisionError> {
    inputs.valid_baseline()?;
    if inputs.korean_integrity_failed {
        return Ok(inputs.decision("en", Source::Baseline, false));
    }
    Ok(choose(inputs))
}

/// Apply the CLI argument row before the common stored/OS/baseline table.
pub fn decide_cli(inputs: &Inputs<'_>, argument: Option<&str>) -> Result<Decision, DecisionError> {
    inputs.valid_baseline()?;
    let argument = argument
        .map(|argument| primary_language(argument).ok_or(DecisionError::UnsupportedLanguage))
        .transpose()?;
    let argument = argument.as_deref();
    if let Some(language) = argument
        && !inputs.contains(language)
        && !(inputs.korean_integrity_failed && language == "ko")
    {
        return Err(DecisionError::UnsupportedLanguage);
    }
    if inputs.korean_integrity_failed {
        return Ok(inputs.decision("en", Source::Baseline, false));
    }
    if let Some(language) = argument {
        return Ok(inputs.decision(language, Source::Argument, false));
    }
    Ok(choose(inputs))
}

fn descriptor(key: &str, default: Value, values: Vec<Value>) -> Descriptor {
    Descriptor {
        presentation: du_core_contracts::settings::SettingPresentation::for_setting(key),
        key: key.to_owned(),
        owner: "F13".to_owned(),
        default,
        kind: ValueKind::Choice(values),
        editable: true,
    }
}

/// `null` represents the absent override. F14 normalizes its default to removal.
/// Use the build's language inventory so an integrity warning does not invalidate
/// a persisted setting. Profile writes must also check the runtime registry.
pub fn descriptors(configured_languages: &[&str]) -> Result<Vec<Descriptor>, DecisionError> {
    let languages: BTreeSet<_> = configured_languages.iter().copied().collect();
    if !languages.contains("en") {
        return Err(DecisionError::RegistrationFailed);
    }
    let mut choices = vec![Value::Null];
    choices.extend(languages.into_iter().map(|language| json!(language)));
    Ok(vec![
        descriptor(DISPLAY_LANGUAGE_KEY, Value::Null, choices),
        descriptor(
            "locale.date_format",
            json!("lang_default"),
            vec![json!("lang_default"), json!("iso8601")],
        ),
        descriptor(
            "locale.number_format",
            json!("lang_default"),
            vec![json!("lang_default"), json!("no_grouping")],
        ),
        descriptor(
            "locale.time_zone_policy",
            json!("local"),
            vec![json!("local"), json!("utc")],
        ),
        descriptor(
            "a11y.motion",
            json!("follow_os"),
            vec![json!("follow_os"), json!("reduce")],
        ),
        descriptor(
            "a11y.app_scale",
            json!(100),
            vec![
                json!(100),
                json!(110),
                json!(125),
                json!(150),
                json!(175),
                json!(200),
            ],
        ),
    ])
}

#[cfg(test)]
mod tests;
