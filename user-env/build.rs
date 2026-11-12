use std::{env, error::Error, fs, path::PathBuf};
#[path = "build/locale_manifest.rs"]
mod locale_manifest;

fn append(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let length = u32::try_from(bytes.len())?;
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = "i18n/resources.txt";
    println!("cargo:rerun-if-changed={manifest}");
    let contents = fs::read_to_string(manifest)?;
    let mut output = Vec::new();
    for line in contents.lines() {
        let mut fields = line.split('|');
        let language = fields
            .next()
            .filter(|s| !s.is_empty())
            .ok_or("missing language")?;
        let version = fields
            .next()
            .filter(|s| !s.is_empty())
            .ok_or("missing version")?;
        let path = fields
            .next()
            .filter(|s| !s.is_empty())
            .ok_or("missing path")?;
        if fields.next().is_some() {
            return Err("unexpected registration field".into());
        }
        println!("cargo:rerun-if-changed={path}");
        let bytes = fs::read(path)?;
        append(&mut output, language.as_bytes())?;
        append(&mut output, version.as_bytes())?;
        append(&mut output, blake3::hash(&bytes).as_bytes())?;
        append(&mut output, &bytes)?;
    }
    let metadata_path = "i18n/reason-messages.json";
    println!("cargo:rerun-if-changed={metadata_path}");
    let metadata = fs::read(metadata_path)?;
    let mut embedded_metadata = Vec::with_capacity(32 + metadata.len());
    embedded_metadata.extend_from_slice(blake3::hash(&metadata).as_bytes());
    embedded_metadata.extend_from_slice(&metadata);
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    fs::write(out.join("bundles.bin"), output)?;
    fs::write(out.join("reason-metadata.bin"), embedded_metadata)?;
    locale_manifest::generate(&out)?;
    Ok(())
}
