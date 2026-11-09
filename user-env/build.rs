use std::{env, error::Error, fs, path::PathBuf};

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
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    fs::write(out.join("bundles.bin"), output)?;
    Ok(())
}
