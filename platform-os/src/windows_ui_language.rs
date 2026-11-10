//! Read the ordered Windows user display-language list without choosing a language.

use std::io;

const MAX_UTF16_UNITS: usize = 65_536;
const INSUFFICIENT_BUFFER: i32 = 122;
const ATTEMPTS: usize = 3;

/// Returns language tags in OS preference order. An empty list is not a language.
/// Failure preserves the OS error when available; callers decide the fallback.
#[cfg(windows)]
pub fn preferred_ui_languages() -> io::Result<Vec<String>> {
    read_with(native_query)
}

fn read_with(
    mut query: impl FnMut(Option<&mut [u16]>) -> io::Result<(u32, u32)>,
) -> io::Result<Vec<String>> {
    for _ in 0..ATTEMPTS {
        let (_, needed) = query(None)?;
        let length = usize::try_from(needed).map_err(|_| invalid_data())?;
        if !(2..=MAX_UTF16_UNITS).contains(&length) {
            return Err(invalid_data());
        }
        let mut buffer = vec![0_u16; length];
        match query(Some(&mut buffer)) {
            Ok((count, used)) => {
                let used = usize::try_from(used).map_err(|_| invalid_data())?;
                return parse(buffer.get(..used).ok_or_else(invalid_data)?, count);
            }
            Err(error) if error.raw_os_error() == Some(INSUFFICIENT_BUFFER) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::from_raw_os_error(INSUFFICIENT_BUFFER))
}

fn invalid_data() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Windows UI language list",
    )
}

fn parse(buffer: &[u16], count: u32) -> io::Result<Vec<String>> {
    if buffer.len() < 2 || !buffer.ends_with(&[0, 0]) {
        return Err(invalid_data());
    }
    let mut languages = Vec::new();
    let mut remaining = &buffer[..buffer.len() - 1];
    while !remaining.is_empty() && remaining[0] != 0 {
        let end = remaining
            .iter()
            .position(|value| *value == 0)
            .ok_or_else(invalid_data)?;
        let tag = String::from_utf16(&remaining[..end]).map_err(|_| invalid_data())?;
        languages.push(tag);
        remaining = &remaining[end + 1..];
    }
    if remaining.iter().any(|value| *value != 0) || languages.len() != count as usize {
        return Err(invalid_data());
    }
    Ok(languages)
}

#[cfg(windows)]
fn native_query(buffer: Option<&mut [u16]>) -> io::Result<(u32, u32)> {
    const MUI_LANGUAGE_NAME: u32 = 8;
    let mut count = 0_u32;
    let (pointer, mut length) = match buffer {
        Some(buffer) => (
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).map_err(|_| invalid_data())?,
        ),
        None => (std::ptr::null_mut(), 0),
    };
    // SAFETY: count and length are writable; pointer is null for a size query,
    // or points to a live writable buffer of exactly length UTF-16 units.
    let result =
        unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, pointer, &mut length) };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok((count, length))
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetUserPreferredUILanguages(
        flags: u32,
        count: *mut u32,
        buffer: *mut u16,
        length: *mut u32,
    ) -> i32;
}

#[cfg(test)]
mod tests;
