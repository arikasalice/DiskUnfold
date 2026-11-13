#![cfg(windows)]
//! Controlled NTFS junction observation; silence does not prove mount coverage.
use du_platform_os::{windows_watch_decode::Completion, windows_watch_native::NativeWatch};
use std::{
    fs,
    process::Command,
    thread,
    time::{Duration, Instant},
};

#[test]
fn recursive_watch_compares_direct_and_junction_target_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path().join("root");
    let outside = fixture.path().join("outside");
    let junction = root.join("mount");
    fs::create_dir(&root)?;
    fs::create_dir(&outside)?;
    let status = Command::new("cmd.exe")
        .current_dir(fixture.path())
        .args(["/D", "/C", "mklink", "/J", "root\\mount"])
        .arg(&outside)
        .status()?;
    if !status.success() {
        return Err("could not create junction".into());
    }
    let result = (|| -> Result<(bool, bool), Box<dyn std::error::Error>> {
        let mut watch = NativeWatch::start(&root)?;
        fs::write(outside.join("outside-change"), b"x")?;
        let end = Instant::now() + Duration::from_millis(800);
        let mut outside_seen = false;
        while Instant::now() < end {
            match watch.poll()? {
                Some(Completion::Changes(changes)) => {
                    outside_seen |= !changes.is_empty();
                }
                Some(other) => {
                    return Err(format!("outside observation inconclusive: {other:?}").into());
                }
                None => thread::sleep(Duration::from_millis(10)),
            }
        }
        fs::write(root.join("direct-change"), b"x")?;
        let end = Instant::now() + Duration::from_secs(10);
        let mut direct_seen = false;
        while Instant::now() < end {
            match watch.poll()? {
                Some(Completion::Changes(changes)) => {
                    let expected: Vec<u8> = "direct-change"
                        .encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect();
                    direct_seen |= changes
                        .iter()
                        .any(|change| change.relative_path == expected);
                    if direct_seen {
                        break;
                    }
                }
                Some(other) => {
                    return Err(format!("direct observation inconclusive: {other:?}").into());
                }
                None => thread::sleep(Duration::from_millis(10)),
            }
        }
        watch.stop()?;
        Ok((outside_seen, direct_seen))
    })();
    // Delete the junction itself only, never traverse or recursively delete it.
    fs::remove_dir(&junction)?;
    let (outside_seen, direct_seen) = result?;
    println!(
        "recursive junction target event observed={outside_seen}; direct event observed={direct_seen}"
    );
    assert!(
        direct_seen,
        "positive control did not generate a notification"
    );
    // This fixture makes no product support claim from either observation.
    Ok(())
}
