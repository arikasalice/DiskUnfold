//! Asks Windows whether a shown path names the same target as its extended form.
//!
//! `std::path::absolute` returns a `\\?\` path unchanged and passes every other
//! path to `GetFullPathNameW`, the Win32 normalization that the extended form
//! skips. Win32 file functions hand a plain path `C:\x` to the object manager
//! as `\??\C:\x` and `\\server\share` as `\??\UNC\server\share`, and an
//! extended path `\\?\R` as `\??\R`. A shown path therefore names the same
//! target exactly when normalization leaves it unchanged and adding the prefix
//! back gives the original. Only strings are compared: no file is opened and no
//! network share is contacted. `GetFullPathNameW` itself would trim `\\?\C:\x.`
//! to `\\?\C:\x`, so it is never asked about the extended form.
use std::path::absolute;

use super::rules::extended;
use crate::display_path::windows;

/// Windows' normalization of a plain drive or UNC path.
fn normalized(plain: &str) -> Result<String, String> {
    absolute(plain)
        .map_err(|error| format!("{plain:?}: {error}"))?
        .into_os_string()
        .into_string()
        .map_err(|text| format!("{plain:?}: not Unicode: {text:?}"))
}

/// The input with its `UNC` marker in canonical case. Win32 file functions look
/// up `\??\UNC` without regard to case; string normalization cannot show that.
fn canonical(input: &str) -> String {
    match input.get(4..8) {
        Some(marker) if marker.eq_ignore_ascii_case(r"UNC\") => format!(r"\\?\UNC\{}", &input[8..]),
        _ => input.to_owned(),
    }
}

/// Checks the rule's result for `input` against Windows; returns whether the
/// prefix was removed.
fn checked(input: &str) -> Result<bool, String> {
    let shown = windows(input);
    if shown == input {
        return Ok(false);
    }
    let os = normalized(&shown)?;
    if os != shown {
        return Err(format!(
            "{input:?} is shown as {shown:?}, which Windows reads as {os:?}"
        ));
    }
    if extended(&shown) != canonical(input) {
        return Err(format!(
            "{input:?} is shown as {shown:?}, which does not round-trip"
        ));
    }
    Ok(true)
}

/// The plain form the rule would show for `input`, before any check.
fn stripped(input: &str) -> String {
    match input.strip_prefix(r"\\?\UNC\") {
        Some(network) => format!(r"\\{network}"),
        None => input.trim_start_matches(r"\\?\").to_owned(),
    }
}

#[test]
fn drive_and_unc_paths_shown_without_the_prefix_name_the_same_target() -> Result<(), String> {
    for input in [
        r"\\?\C:\",
        r"\\?\C:\Users\x",
        r"\\?\z:\lower\",
        r"\\?\C:\파일.txt\😀",
        r"\\?\C:\CONSOLE\COM10\a.NUL\.con\nul-file.txt",
        r"\\?\UNC\server\share",
        r"\\?\UNC\server\share\",
        r"\\?\UNC\server\share\dir\file.txt",
        r"\\?\unc\server\share\x",
        r"\\?\Unc\server.example\share$\x",
    ] {
        if !checked(input)? {
            return Err(format!("{input:?} kept its prefix"));
        }
    }
    Ok(())
}

/// Each plain form below is read differently by Windows on every supported
/// release; the rule keeps the prefix for all of them. Device names other than
/// `NUL` in the last component (`CON`, `COM1`, `NUL.txt`) depend on the
/// release, so the rule tests cover them without asking the OS.
#[test]
fn paths_windows_would_rewrite_keep_the_prefix() -> Result<(), String> {
    for input in [
        r"\\?\C:\trailing.",
        r"\\?\C:\space ",
        r"\\?\C:\trailing.\file",
        r"\\?\C:\a\.\b",
        r"\\?\C:\a\..",
        r"\\?\C:\a//b",
        r"\\?\C:\mixed/slash",
        r"\\?\C:\a\\b",
        r"\\?\C:\NUL",
        r"\\?\C:\a\nul ",
        r"\\?\UNC\server\share\trailing.",
        r"\\?\UNC\server\share\..",
        r"\\?\unc\server\share\space ",
    ] {
        let plain = stripped(&canonical(input));
        let os = normalized(&plain)?;
        if os == plain {
            return Err(format!("Windows reads {plain:?} unchanged"));
        }
        if windows(input) != input {
            return Err(format!(
                "{input:?} lost its prefix, but Windows reads it as {os:?}"
            ));
        }
    }
    Ok(())
}

/// Windows normalizes long plain paths without changing them, so the
/// `MAX_PATH` bound is the only reason these keep the prefix: a program
/// without long-path support could not open the plain form.
#[test]
fn the_length_bound_is_the_only_reason_long_paths_keep_the_prefix() -> Result<(), String> {
    let drive = |n: usize| format!(r"\\?\C:\{}", "a".repeat(n));
    let unc = |n: usize| format!(r"\\?\UNC\s\h\{}", "b".repeat(n));
    // 259 and 248 units: plain forms shown, including directories of 248 or more.
    for input in [drive(256), drive(245), unc(253)] {
        if !checked(&input)? {
            return Err(format!("{} units kept the prefix", input.len()));
        }
    }
    for input in [drive(257), drive(300), unc(254)] {
        let plain = stripped(&input);
        if normalized(&plain)? != plain {
            return Err(format!("Windows rewrote a {}-unit path", plain.len()));
        }
        if windows(&input) != input {
            return Err(format!("{}-unit plain form was shown", plain.len()));
        }
    }
    Ok(())
}

/// Components with names Windows keeps, rewrites or maps to a device.
const NAMES: [&str; 19] = [
    "a",
    "파일.txt",
    "😀",
    "x.y",
    ".hidden",
    "a b",
    "CONSOLE",
    "COM10",
    "nul-file.txt",
    "a.NUL",
    "NUL",
    "nul ",
    "trailing.",
    "space ",
    ".",
    "..",
    "",
    "x/y",
    "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL",
];

/// Every drive and UNC path of up to three components from `NAMES`: each
/// path the rule shows must name the same target according to Windows.
#[test]
fn every_generated_path_shown_names_the_same_target() -> Result<(), String> {
    let mut tails = vec![String::new()];
    let mut level = vec![String::new()];
    for _ in 0..3 {
        level = level
            .iter()
            .flat_map(|tail| NAMES.iter().map(move |name| format!(r"{tail}\{name}")))
            .collect();
        tails.extend(level.iter().cloned());
    }
    let (mut shown, mut rewritten) = (0, 0);
    for (index, tail) in tails.iter().enumerate() {
        let marker = if index % 2 == 0 { "UNC" } else { "unc" };
        for input in [
            format!(r"\\?\C:{tail}"),
            format!(r"\\?\{marker}\server\share{tail}"),
        ] {
            if checked(&input)? {
                shown += 1;
            } else {
                let plain = stripped(&canonical(&input));
                rewritten += usize::from(normalized(&plain)? != plain);
            }
        }
    }
    // The corpus exercises both outcomes in quantity.
    if shown < 1000 || rewritten < 1000 {
        return Err(format!("shown {shown}, rewritten {rewritten}"));
    }
    Ok(())
}
