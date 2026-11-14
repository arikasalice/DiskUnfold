//! Decides whether a Win32 path without the `\\?\` prefix names the same path.
//!
//! Win32 normalizes a path that lacks the prefix before the object manager
//! sees it, and the extended form skips that step. Microsoft's "Naming Files,
//! Paths, and Namespaces" and ".NET file path formats on Windows systems"
//! describe the rules applied here: `/` becomes a separator, separator runs
//! collapse, `.` and `..` components are resolved, trailing dots and spaces
//! are trimmed, reserved device names select a device even with an extension
//! or trailing spaces, and paths of `MAX_PATH` units or more need long-path
//! support. A path that any rule could change keeps its prefix. The checks are
//! deliberately broader than any one Windows release: a kept prefix only costs
//! readability, while a removed one could name a different file.

/// `MAX_PATH` in UTF-16 code units, counting the terminating NUL, so the
/// longest plain form shown is 259 units. Win32 also limits created folder
/// paths to 248 units; that limit governs creation, not which path the text
/// names, so the display rule uses this single bound.
const MAX_PATH: usize = 260;

/// Device names reserved in every component, compared without regard to ASCII case.
const DEVICES: [&str; 6] = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];

/// Port names reserved when followed by one digit or superscript digit.
const PORTS: [&str; 2] = ["COM", "LPT"];

/// Characters Win32 treats as a separator, wildcard or stream marker, or
/// rejects in a name. Control characters below U+0020 are checked separately.
const SPECIAL: &[char] = &['<', '>', ':', '"', '/', '|', '?', '*'];

/// Returns whether `plain` names the same path as its extended form.
///
/// `plain` is the complete drive (`C:\...`) or UNC (`\\server\...`) form;
/// `names` is the part after the drive root or after the leading `\\`. One
/// final separator is allowed; every other component must be a name that
/// Win32 leaves as it is.
pub(super) fn same_without_prefix(plain: &str, names: &str) -> bool {
    plain.encode_utf16().count() < MAX_PATH && components_kept(names)
}

fn components_kept(names: &str) -> bool {
    let mut parts = names.split('\\').peekable();
    while let Some(part) = parts.next() {
        let last = parts.peek().is_none();
        if part.is_empty() && last {
            continue;
        }
        if !name_kept(part) {
            return false;
        }
    }
    true
}

/// A component Win32 neither trims, resolves, splits nor maps to a device.
fn name_kept(name: &str) -> bool {
    !name.is_empty()
        && !name.ends_with(['.', ' '])
        && !name.chars().any(|c| c < ' ' || SPECIAL.contains(&c))
        && !reserved(name)
}

/// A reserved device name, alone or followed by spaces or an extension
/// (`NUL.txt`, `con .tar.gz`). Surrounding spaces are ignored on both sides.
fn reserved(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or(name).trim_matches(' ');
    DEVICES
        .iter()
        .any(|device| base.eq_ignore_ascii_case(device))
        || port(base)
}

/// `COM` or `LPT` followed by `0`-`9`, `¹`, `²` or `³`. `COM0` and
/// `LPT0` are included because current Microsoft documentation lists them.
fn port(base: &str) -> bool {
    let (Some(stem), Some(digit)) = (base.get(..3), base.get(3..)) else {
        return false;
    };
    let mut digits = digit.chars();
    PORTS.iter().any(|port| stem.eq_ignore_ascii_case(port))
        && digits
            .next()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, '¹' | '²' | '³'))
        && digits.next().is_none()
}
