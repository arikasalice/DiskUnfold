//! Validates and interprets the published named-placeholder syntax.

use std::collections::BTreeSet;

use super::RegistrationError;

pub(super) enum Part<'a> {
    Text(&'a str),
    Parameter(&'a str),
}

pub(super) fn parse(source: &str) -> Result<Vec<Part<'_>>, RegistrationError> {
    let mut parts = Vec::new();
    let mut rest = source;
    while let Some(index) = rest.find(['{', '}']) {
        let (prefix, following) = rest.split_at(index);
        if !prefix.is_empty() {
            parts.push(Part::Text(prefix));
        }
        if let Some(next) = following.strip_prefix("{{") {
            parts.push(Part::Text("{"));
            rest = next;
        } else if let Some(next) = following.strip_prefix("}}") {
            parts.push(Part::Text("}"));
            rest = next;
        } else if let Some(open) = following.strip_prefix('{') {
            let (name, next) = open.split_once('}').ok_or(RegistrationError)?;
            if !parameter_name(name) {
                return Err(RegistrationError);
            }
            parts.push(Part::Parameter(name));
            rest = next;
        } else {
            return Err(RegistrationError);
        }
    }
    if !rest.is_empty() {
        parts.push(Part::Text(rest));
    }
    Ok(parts)
}

fn parameter_name(value: &str) -> bool {
    let mut chars = value.bytes();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == b'_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
}

pub(super) fn validate<'a>(
    texts: impl IntoIterator<Item = &'a str>,
    selector: Option<&'a str>,
    declared: impl IntoIterator<Item = &'a str>,
) -> Result<(), RegistrationError> {
    let mut used: BTreeSet<_> = selector.into_iter().collect();
    for text in texts {
        if text.is_empty() {
            return Err(RegistrationError);
        }
        used.extend(parse(text)?.into_iter().filter_map(|part| match part {
            Part::Text(_) => None,
            Part::Parameter(name) => Some(name),
        }));
    }
    let expected: BTreeSet<_> = declared.into_iter().collect();
    if used != expected {
        return Err(RegistrationError);
    }
    Ok(())
}

pub(super) fn render(
    source: &str,
    mut parameter: impl FnMut(&str) -> String,
) -> Result<String, RegistrationError> {
    let mut output = String::new();
    for part in parse(source)? {
        match part {
            Part::Text(text) => output.push_str(text),
            Part::Parameter(name) => output.push_str(&parameter(name)),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_braces_and_rejects_invalid_parameter_syntax() -> Result<(), RegistrationError> {
        assert_eq!(render("{{{name}}}", |_| "file".to_owned())?, "{file}");
        assert!(parse("{unclosed").is_err());
        assert!(parse("unpaired }").is_err());
        assert!(validate(["{extra}"], None, ["name"]).is_err());
        Ok(())
    }
}
