//! Key encodings shared with the platform identity adapters.
//!
//! Windows release is 10 or 11 followed by DisplayVersion; macOS release is the
//! major version and the edition is macos; Linux release is VERSION_ID and the
//! edition is the os-release ID.

/// Returns true for YYYY-MM-DD with plausible month and day.
pub(super) fn is_date(value: &str) -> bool {
    let b = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !digits(0..4)
        || !digits(5..7)
        || !digits(8..10)
    {
        return false;
    }
    let month: u32 = value[5..7].parse().unwrap_or(0);
    let day: u32 = value[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

/// Validates the identity keys of one combination.
pub(super) fn validate_key(
    os: &str,
    release: &str,
    edition: &str,
    cpu: &str,
) -> Result<(), String> {
    if !matches!(cpu, "x64" | "arm64") {
        return Err(format!("cpu {cpu:?} is not x64 or arm64"));
    }
    let ok = match os {
        "windows" => {
            release
                .split_once('-')
                .is_some_and(|(major, display)| matches!(major, "10" | "11") && token(display))
                && token(edition)
        }
        "macos" => {
            !release.is_empty() && release.bytes().all(|b| b.is_ascii_digit()) && edition == "macos"
        }
        "linux" => {
            token(release)
                && edition
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
                && !edition.is_empty()
        }
        _ => return Err(format!("os {os:?} is not windows, macos or linux")),
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{os} key {release:?}/{edition:?} does not follow the adopted encoding"
        ))
    }
}
