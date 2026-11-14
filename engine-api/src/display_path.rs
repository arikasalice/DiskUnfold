//! User-facing text for native paths in engine and host responses.
//!
//! Every complete path shown to a person passes through this module. Only
//! display text changes: raw bytes, comparison keys, fingerprints, exclusion
//! keys, stored bodies and item references keep their native form, and no
//! caller may reopen or compare a path from the text returned here.
//!
//! A Windows path loses its `\\?\` prefix only when the plain form names the
//! same path under Win32 rules: restoring the prefix gives back the original
//! text, and Win32 normalization leaves the plain form unchanged. Every other
//! path is shown with its prefix; the `win32` submodule lists the rules.
//!
//! CLI and MCP outputs never reuse this text. When they export paths, they
//! derive their own channel form from the raw bytes.
use std::borrow::Cow;

use du_core_contracts::{
    OsPath, PathEncoding,
    analysis_ipc::{RecentScopeOpen, ScopeDraft, ScopeExclusion, ScopeReview},
};

const VERBATIM: &str = r"\\?\";
const UNC: &str = r"UNC\";

mod win32;

/// Decodes one native name component for display, without path rules.
///
/// Unix bytes and unpaired UTF-16 surrogates decode lossily. Returns `None`
/// when Windows UTF-16LE bytes have an odd length.
pub(crate) fn name(encoding: &PathEncoding, bytes: &[u8]) -> Option<String> {
    match encoding {
        PathEncoding::Unix => Some(String::from_utf8_lossy(bytes).into_owned()),
        PathEncoding::WindowsUtf16le => {
            let (pairs, remainder) = bytes.as_chunks::<2>();
            if !remainder.is_empty() {
                return None;
            }
            let units: Vec<_> = pairs.iter().copied().map(u16::from_le_bytes).collect();
            Some(String::from_utf16_lossy(&units))
        }
    }
}

/// Decodes a complete native path and applies the display rule of its encoding.
///
/// Returns `None` when Windows UTF-16LE bytes have an odd length.
pub(crate) fn path(encoding: &PathEncoding, bytes: &[u8]) -> Option<String> {
    let decoded = name(encoding, bytes)?;
    Some(match encoding {
        PathEncoding::Unix => decoded,
        PathEncoding::WindowsUtf16le => windows(&decoded).into_owned(),
    })
}

/// Removes the Win32 extended-length prefix when the plain form names the same path.
///
/// `\\?\C:\x` becomes `C:\x` and `\\?\UNC\server\share\x` becomes
/// `\\server\share\x`; the `UNC` marker is matched without regard to case.
/// The prefix stays when Win32 would read the plain form differently: a
/// component ending in `.` or space, a `.` or `..` component, an empty
/// component, a reserved device name, a character that Win32 treats as a
/// separator, wildcard or stream marker, or a plain form of `MAX_PATH` UTF-16
/// units or more. Volume GUID, `GLOBALROOT`, `\\.\` device and every other
/// form stay unchanged.
///
/// Guarantee: when the result differs from `text`, adding the prefix back
/// yields `text` (with `UNC` in its canonical case) and both forms name the
/// same path. The result is display text; callers must not reopen it.
pub(crate) fn windows(text: &str) -> Cow<'_, str> {
    let Some(rest) = text.strip_prefix(VERBATIM) else {
        return Cow::Borrowed(text);
    };
    if let [letter, b':', b'\\', ..] = rest.as_bytes()
        && letter.is_ascii_alphabetic()
    {
        return plain_if_same(text, Cow::Borrowed(rest), &rest[3..]);
    }
    let network = rest
        .get(..UNC.len())
        .filter(|marker| marker.eq_ignore_ascii_case(UNC))
        .map(|_| &rest[UNC.len()..]);
    match network {
        Some(network) if server_and_share(network) => {
            plain_if_same(text, Cow::Owned(format!(r"\\{network}")), network)
        }
        _ => Cow::Borrowed(text),
    }
}

/// Returns `plain` when it names the same path as `original`; `names` holds
/// the components after the drive root or the leading `\\` of a UNC path.
fn plain_if_same<'a>(original: &'a str, plain: Cow<'a, str>, names: &str) -> Cow<'a, str> {
    if win32::same_without_prefix(&plain, names) {
        plain
    } else {
        Cow::Borrowed(original)
    }
}

fn server_and_share(network: &str) -> bool {
    let mut parts = network.splitn(3, '\\');
    [parts.next(), parts.next()]
        .into_iter()
        .all(|part| part.is_some_and(|part| !part.is_empty() && part != "." && part != ".."))
}

/// Replaces the display text of a Windows path; other fields stay unchanged.
///
/// `lossy` keeps its value because the removed prefix is ASCII.
pub(crate) fn os_path(mut path: OsPath) -> OsPath {
    project_os_path(&mut path);
    path
}

fn project_os_path(path: &mut OsPath) {
    if path.encoding != PathEncoding::WindowsUtf16le {
        return;
    }
    let projected = windows(&path.display);
    if projected != path.display.as_str() {
        path.display = projected.into_owned();
    }
}

fn project_draft(draft: &mut ScopeDraft) {
    draft.includes.iter_mut().for_each(project_os_path);
    draft.included_mounts.iter_mut().for_each(project_os_path);
    for rule in &mut draft.exclusions {
        if let ScopeExclusion::Path { path } = rule {
            project_os_path(path);
        }
    }
}

/// Projects every path shown from a scope review response.
pub(crate) fn scope_review(mut review: ScopeReview) -> ScopeReview {
    project_draft(&mut review.scope);
    for target in &mut review.targets {
        project_os_path(&mut target.path);
    }
    review
}

/// Projects every path shown from a restored recent scope response.
pub fn recent_scope_open(mut open: RecentScopeOpen) -> RecentScopeOpen {
    project_draft(&mut open.draft);
    for target in &mut open.targets {
        project_os_path(&mut target.path);
        if let Some(entry) = &mut target.proposed_entry {
            project_os_path(entry);
        }
    }
    open.review = open.review.map(scope_review);
    open
}

#[cfg(test)]
mod tests;
