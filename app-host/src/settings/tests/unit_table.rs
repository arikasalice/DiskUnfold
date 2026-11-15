//! The histogram unit table follows the stored F03 display unit system.
use super::*;

fn base_for(path: &Path, stored: Option<&str>) -> Result<u16, Box<dyn std::error::Error>> {
    if let Some(system) = stored {
        let mut db = AppDb::create_gui(path).map_err(|e| format!("{e:?}"))?;
        db.commit(
            0,
            &SettingsChanges::from([(space::SIZE_UNIT_SYSTEM.into(), Some(json!(system)))]),
            context(),
        )
        .map_err(|e| format!("{e:?}"))?;
    }
    let settings = Arc::new(StartupSettings::open(
        &test_database(path)?,
        100,
        &["en", "ko"],
    )?);
    Ok((settings.unit_table_source())().base())
}

#[test]
fn the_unit_table_follows_the_display_unit_system() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    // A fresh store uses the system this platform registers as its default.
    let default = if cfg!(windows) { 1024 } else { 1000 };
    assert_eq!(base_for(&dir.path().join("fresh.db"), None)?, default);
    assert_eq!(base_for(&dir.path().join("si.db"), Some("si"))?, 1000);
    assert_eq!(base_for(&dir.path().join("iec.db"), Some("iec"))?, 1024);
    Ok(())
}
