//! Reads the installed WebView2 runtime version from the registry.
//!
//! The Evergreen runtime records its version as the `pv` value under the
//! EdgeUpdate client key, per machine (64-bit and 32-bit views) or per user.
use super::{
    WebViewAvailability,
    webview_runtime::{RuntimeProbe, availability},
};
use ::windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
        },
    },
    core::{PCWSTR, w},
};

/// Longest version text accepted, in bytes.
const MAX_VALUE_BYTES: u32 = 256;

const MACHINE_64: PCWSTR = w!(
    "SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
);
const MACHINE_32: PCWSTR =
    w!("SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}");
const USER: PCWSTR =
    w!("Software\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}");

/// Reports whether an Evergreen WebView2 runtime is registered on this system.
pub(super) fn webview_availability() -> WebViewAvailability {
    let probes = [
        probe(HKEY_LOCAL_MACHINE, MACHINE_64),
        probe(HKEY_LOCAL_MACHINE, MACHINE_32),
        probe(HKEY_CURRENT_USER, USER),
    ];
    let override_present = std::env::var_os("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER")
        .is_some_and(|value| !value.is_empty());
    availability(&probes, override_present)
}

fn probe(root: HKEY, subkey: PCWSTR) -> RuntimeProbe {
    let mut bytes = 0_u32;
    // SAFETY: both strings are NUL-terminated constants; a null data pointer
    // with an out size is the documented size query and writes only `bytes`.
    let status = unsafe {
        RegGetValueW(
            root,
            subkey,
            w!("pv"),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return RuntimeProbe::Missing;
    }
    if status.is_err() || !(2..=MAX_VALUE_BYTES).contains(&bytes) {
        return RuntimeProbe::Unreadable;
    }
    let Ok(length) = usize::try_from(bytes) else {
        return RuntimeProbe::Unreadable;
    };
    let mut buffer = vec![0_u16; length.div_ceil(2)];
    let Ok(mut capacity) = u32::try_from(buffer.len() * 2) else {
        return RuntimeProbe::Unreadable;
    };
    // SAFETY: `buffer` is writable for `capacity` bytes and outlives the call;
    // the API never writes past the capacity passed in and updates it to the
    // number of bytes written. The strings are the same NUL-terminated constants.
    let status = unsafe {
        RegGetValueW(
            root,
            subkey,
            w!("pv"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut capacity),
        )
    };
    if status.is_err() {
        return RuntimeProbe::Unreadable;
    }
    let written = (usize::try_from(capacity).unwrap_or(0) / 2).min(buffer.len());
    let text = &buffer[..written];
    let end = text
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(text.len());
    String::from_utf16(&text[..end]).map_or(RuntimeProbe::Unreadable, RuntimeProbe::Version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::windows::Win32::System::Registry::{REG_DWORD, REG_SZ, RegDeleteTreeW, RegSetKeyValueW};

    /// A throwaway per-user key that only this test process uses.
    struct ScratchKey(Vec<u16>);

    impl ScratchKey {
        fn new(name: &str) -> Self {
            let path = format!("Software\\DiskUnfoldTests\\{name}-{}", std::process::id());
            Self(path.encode_utf16().chain(Some(0)).collect())
        }

        fn path(&self) -> PCWSTR {
            PCWSTR(self.0.as_ptr())
        }

        fn set(&self, kind: ::windows::Win32::System::Registry::REG_VALUE_TYPE, data: &[u8]) {
            let length = u32::try_from(data.len()).unwrap_or_default();
            // SAFETY: `data` is readable for `length` bytes and the key path is NUL-terminated.
            let status = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    self.path(),
                    w!("pv"),
                    kind.0,
                    Some(data.as_ptr().cast()),
                    length,
                )
            };
            assert!(status.is_ok(), "{status:?}");
        }
    }

    impl Drop for ScratchKey {
        fn drop(&mut self) {
            // SAFETY: the path is NUL-terminated; a failed delete only leaves a test key behind.
            let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, self.path()) };
        }
    }

    fn utf16_bytes(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    #[test]
    fn an_absent_key_is_definitely_missing() {
        let key = ScratchKey::new("absent");
        assert_eq!(probe(HKEY_CURRENT_USER, key.path()), RuntimeProbe::Missing);
    }

    #[test]
    fn a_registered_version_is_read_back_without_the_terminator() {
        let key = ScratchKey::new("version");
        key.set(REG_SZ, &utf16_bytes("131.0.2903.51"));
        assert_eq!(
            probe(HKEY_CURRENT_USER, key.path()),
            RuntimeProbe::Version("131.0.2903.51".to_owned())
        );
        key.set(REG_SZ, &utf16_bytes("0.0.0.0"));
        assert_eq!(
            probe(HKEY_CURRENT_USER, key.path()),
            RuntimeProbe::Version("0.0.0.0".to_owned())
        );
    }

    #[test]
    fn a_value_of_the_wrong_type_or_size_is_unreadable_not_absent() {
        let key = ScratchKey::new("wrong-type");
        key.set(REG_DWORD, &7_u32.to_le_bytes());
        assert_eq!(
            probe(HKEY_CURRENT_USER, key.path()),
            RuntimeProbe::Unreadable
        );
        key.set(REG_SZ, &utf16_bytes(&"1.".repeat(200)));
        assert_eq!(
            probe(HKEY_CURRENT_USER, key.path()),
            RuntimeProbe::Unreadable
        );
    }
}
