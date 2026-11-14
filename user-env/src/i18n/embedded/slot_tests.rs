//! Every declared parameter keeps a placeholder in each form of its message.
//!
//! Registration compares the declared parameters with the placeholders of all
//! plural forms together, so a form that drops a value still registers and the
//! value silently disappears from the rendered copy. The plural selector is the
//! one exception: a form chosen by the count may spell the number out (English
//! "one"), so at least one form of the message must show it.

use std::collections::BTreeSet;

use serde_json::Value;

use super::*;
use crate::i18n::template::{self, Part};

fn placeholders(text: &str) -> Result<BTreeSet<&str>, RegistrationError> {
    Ok(template::parse(text)?
        .into_iter()
        .filter_map(|part| match part {
            Part::Text(_) => None,
            Part::Parameter(name) => Some(name),
        })
        .collect())
}

/// `language key forms parameter` for each declared parameter a form omits.
fn slot_gaps(bundle: &Bundle) -> Result<Vec<String>, RegistrationError> {
    let mut gaps = Vec::new();
    for (key, message) in &bundle.messages {
        let forms: Vec<(&str, &str)> = match &message.plural {
            Some(plural) => plural
                .variants
                .iter()
                .map(|(form, text)| (form.as_str(), text.as_str()))
                .collect(),
            None => vec![("text", message.text.as_str())],
        };
        let selector = message
            .plural
            .as_ref()
            .map(|plural| plural.parameter.as_str());
        let used = forms
            .iter()
            .map(|(form, text)| Ok((*form, placeholders(text)?)))
            .collect::<Result<Vec<_>, RegistrationError>>()?;
        for parameter in message.parameters.keys().map(String::as_str) {
            let missing: Vec<_> = used
                .iter()
                .filter(|(_, names)| !names.contains(parameter))
                .map(|(form, _)| *form)
                .collect();
            let gap = if Some(parameter) == selector {
                missing.len() == used.len()
            } else {
                !missing.is_empty()
            };
            if gap {
                gaps.push(format!(
                    "{} {key} {} {parameter}",
                    bundle.language,
                    missing.join(",")
                ));
            }
        }
    }
    Ok(gaps)
}

/// The built-in bundle with the given plural forms of one message replaced,
/// parsed by the same rules registration applies.
fn with_forms(
    language: &str,
    key: &str,
    forms: &[(&str, &str)],
) -> Result<Bundle, RegistrationError> {
    let resource = resources()?
        .into_iter()
        .find(|resource| resource.language == language)
        .ok_or(RegistrationError)?;
    let mut bundle: Value =
        serde_json::from_slice(resource.bytes).map_err(|_| RegistrationError)?;
    let variants = bundle[5]
        .as_array_mut()
        .and_then(|rows| rows.iter_mut().find(|row| row[0] == key))
        .and_then(|row| row[1]["variants"].as_object_mut())
        .ok_or(RegistrationError)?;
    for (form, text) in forms {
        *variants.get_mut(*form).ok_or(RegistrationError)? = Value::from(*text);
    }
    parse_bundle(&serde_json::to_vec(&bundle).map_err(|_| RegistrationError)?)
}

#[test]
fn built_in_messages_show_every_declared_parameter_in_each_form() -> Result<(), RegistrationError> {
    let registry = register(&[])?;
    for language in ["en", "ko"] {
        let bundle = registry.bundle(language).ok_or(RegistrationError)?;
        assert_eq!(slot_gaps(bundle)?, Vec::<String>::new());
    }
    Ok(())
}

#[test]
fn a_form_without_a_declared_parameter_registers_but_is_reported() -> Result<(), RegistrationError>
{
    let key = "ui.analysis.progress_items_remaining";
    let dropped = with_forms("en", key, &[("one", "{count} item left")])?;
    assert_eq!(slot_gaps(&dropped)?, [format!("en {key} one percent")]);

    let key = "ui.analysis.copied_paths";
    let spelled = with_forms("en", key, &[("one", "Copied one path.")])?;
    assert_eq!(slot_gaps(&spelled)?, Vec::<String>::new());
    let unnumbered = with_forms(
        "en",
        key,
        &[("one", "Copied one path."), ("other", "Copied the paths.")],
    )?;
    assert_eq!(
        slot_gaps(&unnumbered)?,
        [format!("en {key} one,other count")]
    );
    Ok(())
}
