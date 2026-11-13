//! Windows release and edition facts read from `RtlGetVersion` and the registry.
use windows::{
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::System::{
        Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
        SystemInformation::OSVERSIONINFOW,
    },
    core::{PCWSTR, w},
};

/// Longest registry string accepted, in bytes; larger values are treated as unknown.
const MAX_VALUE_BYTES: u32 = 512;

/// Raw Windows version facts; the caller maps them to support-matrix keys.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsVersionFacts {
    /// Major version reported by `RtlGetVersion`.
    pub major: u32,
    /// Build number reported by `RtlGetVersion`.
    pub build: u32,
    /// Registry `DisplayVersion` such as `25H2`, when present.
    pub display_version: Option<String>,
    /// Registry `EditionID` such as `Professional`, when present.
    pub edition_id: Option<String>,
}

/// Reads the version facts; `None` means the OS version itself could not be read.
pub fn read() -> Option<WindowsVersionFacts> {
    let (major, build) = version()?;
    Some(WindowsVersionFacts {
        major,
        build,
        display_version: registry_string(w!("DisplayVersion")),
        edition_id: registry_string(w!("EditionID")),
    })
}

fn version() -> Option<(u32, u32)> {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: u32::try_from(size_of::<OSVERSIONINFOW>()).ok()?,
        ..Default::default()
    };
    // SAFETY: `info` is a live, writable OSVERSIONINFOW whose size field is set;
    // RtlGetVersion writes only within that structure and keeps no pointer to it.
    let status = unsafe { RtlGetVersion(&mut info) };
    status
        .is_ok()
        .then_some((info.dwMajorVersion, info.dwBuildNumber))
}

fn registry_string(value: PCWSTR) -> Option<String> {
    let subkey = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let mut bytes = 0_u32;
    // SAFETY: both strings are NUL-terminated constants; a null data pointer
    // with an out size is the documented size query and writes only `bytes`.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
    }
    .ok()
    .ok()?;
    if !(2..=MAX_VALUE_BYTES).contains(&bytes) {
        return None;
    }
    let mut buffer = vec![0_u16; usize::try_from(bytes).ok()?.div_ceil(2)];
    let mut capacity = u32::try_from(buffer.len() * 2).ok()?;
    // SAFETY: `buffer` is writable for `capacity` bytes and outlives the call;
    // the API never writes past the capacity passed in and updates it to the
    // number of bytes written. The strings are the same NUL-terminated constants.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut capacity),
        )
    }
    .ok()
    .ok()?;
    let written = (usize::try_from(capacity).ok()? / 2).min(buffer.len());
    let text = &buffer[..written];
    let end = text
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(text.len());
    String::from_utf16(&text[..end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_registry_string_and_rejects_a_missing_value() {
        assert!(registry_string(w!("EditionID")).is_some_and(|value| !value.is_empty()));
        assert_eq!(
            registry_string(w!("DiskUnfoldDefinitelyMissingValue")),
            None
        );
    }
}
