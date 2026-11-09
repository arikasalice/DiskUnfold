use du_core_contracts::*;
use std::{env, fs, io};
use ts_rs::TS;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = env::args_os()
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "output directory required"))?;
    fs::create_dir_all(&dir)?;
    let config = ts_rs::Config::default();
    let declarations = [
        Id::decl(&config),
        ByteSize::decl(&config),
        Timestamp::decl(&config),
        SizeBasis::decl(&config),
        SizeEvidence::decl(&config),
        SizeAmount::decl(&config),
        SizeValue::decl(&config),
        PathEncoding::decl(&config),
        PathBytes::decl(&config),
        OsPath::decl(&config),
        ItemRef::decl(&config),
        ErrorCode::decl(&config),
        ErrorEnvelope::decl(&config),
        CallEnvelope::decl(&config),
        ResultEnvelope::decl(&config),
        EventEnvelope::decl(&config),
    ];
    let output = declarations
        .into_iter()
        .map(|d| format!("export {d}\n"))
        .collect::<String>();
    fs::write(std::path::Path::new(&dir).join("contracts.ts"), output)?;
    Ok(())
}
