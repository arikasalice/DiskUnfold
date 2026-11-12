//! Conversion from verified embedded data to the IF-02 wire body.

use std::collections::BTreeMap;

use blake3::Hash;
use du_core_contracts::locale_bundle::{
    LocaleBundleBody, LocaleFormatTable, MessageParameterKind, MessageTemplate, ReasonMessageMeta,
};

use super::{Bundle, Message, RegistrationError};

pub(super) fn find_message(bundle: &Bundle, key: &str) -> Option<MessageTemplate> {
    let message = bundle
        .messages
        .iter()
        .find_map(|(candidate, message)| (candidate == key).then_some(message))?;
    typed_message(message)
}

fn typed_message(message: &Message) -> Option<MessageTemplate> {
    let parameters = message
        .parameters
        .iter()
        .map(|(name, kind)| Some((name.clone(), parse_parameter_kind(kind)?)))
        .collect::<Option<BTreeMap<_, _>>>()?;
    Some(match &message.plural {
        Some(plural) => MessageTemplate::Plural {
            parameters,
            parameter: plural.parameter.clone(),
            variants: plural.variants.clone(),
        },
        None => MessageTemplate::Text {
            parameters,
            template: message.text.clone(),
        },
    })
}

pub(super) fn parse_parameter_kind(value: &str) -> Option<MessageParameterKind> {
    match value {
        "count" => Some(MessageParameterKind::Count),
        "size" => Some(MessageParameterKind::Size),
        "timestamp" => Some(MessageParameterKind::Timestamp),
        "number" => Some(MessageParameterKind::Number),
        "path" => Some(MessageParameterKind::Path),
        "name" => Some(MessageParameterKind::Name),
        "message_key" => Some(MessageParameterKind::MessageKey),
        _ => None,
    }
}

pub(super) fn from_verified_bundle(
    bundle: &Bundle,
    reason_messages: &BTreeMap<String, ReasonMessageMeta>,
    metadata_hash: &Hash,
) -> Result<LocaleBundleBody, RegistrationError> {
    let mut messages = BTreeMap::new();
    for (key, message) in &bundle.messages {
        if messages
            .insert(
                key.clone(),
                typed_message(message).ok_or(RegistrationError)?,
            )
            .is_some()
        {
            return Err(RegistrationError);
        }
    }
    let format_table = serde_json::from_value::<LocaleFormatTable>(bundle.format_table.clone())
        .map_err(|_| RegistrationError)?;
    Ok(LocaleBundleBody {
        messages,
        format_table,
        plural_rule: bundle.plural_rule.clone(),
        fallback_language: (!bundle.fallback_language.is_empty())
            .then(|| bundle.fallback_language.clone()),
        reason_messages: reason_messages.clone(),
        metadata_revision: metadata_hash.to_hex().to_string(),
    })
}
