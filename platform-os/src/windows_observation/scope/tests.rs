//! Scope paths in another encoding than the Windows adapter's.
use super::*;
use du_core_contracts::ports::PortErrorKind;

/// A Unix path starts with the byte `/`, so its first UTF-16LE unit is never
/// a drive letter or `\\`; the path is refused before any handle is opened.
#[test]
fn unix_encoded_paths_are_refused() {
    // Odd and even byte lengths: the even ones decode to non-ASCII names.
    for raw in [
        &b"/"[..],
        b"/data",
        b"/dat",
        b"/tmp/a",
        b"/home/user",
        b"/C:/data",
        b"/tmp/scope\xff",
    ] {
        let refused = WindowsScopeRoots
            .open_root(raw)
            .err()
            .map(|error| (error.kind, error.reason_key));
        assert_eq!(
            refused,
            Some((PortErrorKind::Unsupported, "path.unresolvable".to_owned())),
            "{raw:?}"
        );
    }
}
