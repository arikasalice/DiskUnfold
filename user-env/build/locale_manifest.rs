use std::{collections::BTreeSet, error::Error, fmt::Write, fs, path::Path};

const MANIFEST: &str = "i18n/message-keys.txt";
const KINDS: &[&str] = &[
    "count",
    "size",
    "timestamp",
    "number",
    "path",
    "name",
    "message_key",
];

pub fn generate(output: &Path) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={MANIFEST}");
    let source = fs::read_to_string(MANIFEST)?;
    let mut seen = BTreeSet::new();
    let mut keys = String::from(
        "/// Registered keys required in the embedded baseline.\npub const DECLARED_KEYS: &[&str] = &[\n",
    );
    let mut parameters = String::from(
        "#[cfg(test)]\npub(super) const DECLARED_PARAMETERS: &[(&str, &[(&str, &str)])] = &[\n",
    );
    for line in source.lines() {
        let mut tokens = line.split_whitespace();
        let Some(key) = tokens.next() else { continue };
        if !valid_key(key) || !seen.insert(key) {
            return Err(format!("invalid or duplicate locale key: {key}").into());
        }
        writeln!(keys, "    {key:?},")?;
        let row = parameter_row(tokens)?;
        if !row.is_empty() {
            writeln!(parameters, "    ({key:?}, &[{row}]),")?;
        }
    }
    keys.push_str("];\n");
    parameters.push_str("];\n");
    fs::write(output.join("locale-declarations.rs"), keys + &parameters)?;
    Ok(())
}

fn valid_key(key: &str) -> bool {
    key.bytes()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, b'_' | b'.'))
}

fn parameter_row<'a>(tokens: impl Iterator<Item = &'a str>) -> Result<String, Box<dyn Error>> {
    let mut seen = BTreeSet::new();
    let mut output = String::new();
    for token in tokens {
        let (name, kind) = token
            .split_once(':')
            .ok_or("invalid locale parameter declaration")?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
            || !KINDS.contains(&kind)
            || !seen.insert(name)
        {
            return Err(format!("invalid or duplicate locale parameter: {token}").into());
        }
        write!(output, "({name:?}, {kind:?}),")?;
    }
    Ok(output)
}
