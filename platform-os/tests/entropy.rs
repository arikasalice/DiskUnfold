use du_core_contracts::ports::Random;
use du_platform_os::entropy::OsRandom;

#[test]
fn os_random_fills_session_sized_buffers() -> Result<(), String> {
    let mut first = [0; 32];
    let mut second = [0; 32];
    OsRandom.fill(&mut first).map_err(|e| format!("{e:?}"))?;
    OsRandom.fill(&mut second).map_err(|e| format!("{e:?}"))?;
    assert_ne!(first, second);
    Ok(())
}
