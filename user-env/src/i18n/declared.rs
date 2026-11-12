//! Immutable key and parameter declarations from the embedded locale inventory.

use std::{collections::BTreeMap, sync::LazyLock};

use super::{RegistrationError, bundle_body::parse_parameter_kind};

type Declarations = BTreeMap<String, BTreeMap<String, String>>;
const MANIFEST: &str = include_str!("../../i18n/message-keys.txt");
static DECLARATIONS: LazyLock<Result<Declarations, RegistrationError>> =
    LazyLock::new(|| parse(MANIFEST));

pub(super) fn declarations() -> Result<&'static Declarations, RegistrationError> {
    DECLARATIONS.as_ref().map_err(|_| RegistrationError)
}

fn parse(source: &str) -> Result<Declarations, RegistrationError> {
    let mut output = BTreeMap::new();
    for line in source.lines() {
        let mut tokens = line.split_whitespace();
        let Some(key) = tokens.next() else { continue };
        if !key
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'.'))
            || output
                .insert(key.to_owned(), parameter_row(tokens)?)
                .is_some()
        {
            return Err(RegistrationError);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_case_sensitive_message_keys() -> Result<(), RegistrationError> {
        let manifest = parse("ui.visualization.viewMembersOf name:name")?;
        assert!(manifest.contains_key("ui.visualization.viewMembersOf"));
        assert!(!manifest.contains_key("ui.visualization.viewmembersof"));
        Ok(())
    }
}

fn parameter_row<'a>(
    tokens: impl Iterator<Item = &'a str>,
) -> Result<BTreeMap<String, String>, RegistrationError> {
    let mut output = BTreeMap::new();
    for token in tokens {
        let (name, kind) = token.split_once(':').ok_or(RegistrationError)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
            || parse_parameter_kind(kind).is_none()
            || output.insert(name.to_owned(), kind.to_owned()).is_some()
        {
            return Err(RegistrationError);
        }
    }
    Ok(output)
}
