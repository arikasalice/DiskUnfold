use super::*;
use crate::windows_observation::WindowsScopeRoots;
use du_core_contracts::{ports::PortErrorKind, scan_observation::ScopeRootPort};
use std::{
    fs,
    os::windows::ffi::OsStrExt,
    thread,
    time::{Duration, Instant},
};

#[test]
fn scanned_root_reports_direct_change_and_stops() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path().join("root");
    fs::create_dir(&root)?;
    fs::create_dir(root.join("child"))?;
    let native: Vec<u8> = root
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    let scan = WindowsScopeRoots
        .open_root(&native)
        .map_err(|e| format!("{e:?}"))?;
    let bad = RootWatch::start(&root, &scan.volume_id, &[0; 8]);
    assert!(matches!(bad, Err(Error::Identity(e)) if e.kind == PortErrorKind::IdentityChanged));
    let mut watch =
        RootWatch::start(&root, &scan.volume_id, &scan.file_id).map_err(|e| format!("{e:?}"))?;
    assert!(fs::rename(&root, fixture.path().join("other")).is_err());
    fs::write(root.join("child").join("nested"), b"x")?;
    let until = Instant::now() + Duration::from_millis(200);
    while Instant::now() < until {
        assert!(
            watch.poll(1)?.is_none(),
            "root-only watch reported a nested change"
        );
        thread::sleep(Duration::from_millis(10));
    }
    fs::write(root.join("direct"), b"x")?;
    let until = Instant::now() + Duration::from_secs(10);
    let event = loop {
        if let Some(event) = watch.poll(2)? {
            break event;
        }
        if Instant::now() >= until {
            return Err("root notification timed out".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(matches!(
        event,
        WatchEvent::Signal {
            path: None,
            hint: ChangeHint::Unknown,
            observed_at: 2
        }
    ));
    watch.stop()?;
    fs::rename(&root, fixture.path().join("other"))?;
    Ok(())
}
