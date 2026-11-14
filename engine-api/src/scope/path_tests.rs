//! Native scope path validation for both published encodings.
use super::native_path;
use du_core_contracts::{OsPath, PathBytes, PathEncoding};

fn path(encoding: PathEncoding, raw: &[u8]) -> OsPath {
    OsPath {
        encoding,
        raw: PathBytes::from_bytes(raw),
        display: "untrusted label".into(),
        lossy: false,
    }
}

#[test]
fn unix_paths_are_absolute_raw_bytes_without_nul() {
    let checked = native_path(&path(PathEncoding::Unix, b"/home/a\xff"));
    let checked = checked.ok();
    assert_eq!(
        checked.as_ref().map(|p| (p.display.as_str(), p.lossy)),
        Some(("/home/a\u{fffd}", true))
    );
    assert_eq!(
        checked.map(|p| p.raw),
        Some(PathBytes::from_bytes(b"/home/a\xff"))
    );
    for raw in [
        &b""[..],
        b"relative/a",
        b"/with\0nul",
        b"/..",
        b"/home/../etc",
        b"/home/a/..",
    ] {
        assert!(
            native_path(&path(PathEncoding::Unix, raw)).is_err(),
            "{raw:?}"
        );
    }
    // Empty and `.` names stay as given; the adapter and the keys skip them.
    for raw in [&b"//home/./a/"[..], b"/home/a/.", b"/home/..a/b.."] {
        let checked = native_path(&path(PathEncoding::Unix, raw)).map(|p| p.raw);
        assert_eq!(checked, Ok(PathBytes::from_bytes(raw)), "{raw:?}");
    }
}

#[test]
fn windows_paths_keep_their_existing_validation() {
    let wide: Vec<u8> = "C:\\Fixture"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let checked = native_path(&path(PathEncoding::WindowsUtf16le, &wide)).ok();
    assert_eq!(checked.map(|p| p.display), Some("C:\\Fixture".to_owned()));
    assert!(native_path(&path(PathEncoding::WindowsUtf16le, b"odd")).is_err());
}
