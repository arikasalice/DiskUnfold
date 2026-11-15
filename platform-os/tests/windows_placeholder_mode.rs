#![cfg(windows)]
#![allow(unsafe_code)]
//! Runs in its own process: the placeholder compatibility mode is process-wide.
use du_platform_os::windows_placeholder_mode::disguise_placeholders;
use windows::Wdk::Storage::FileSystem::{
    RtlQueryProcessPlaceholderCompatibilityMode, RtlSetProcessPlaceholderCompatibilityMode,
};

/// `PHCM_DISGUISE_PLACEHOLDER` and `PHCM_EXPOSE_PLACEHOLDERS` from `ntifs.h`.
const DISGUISE: i8 = 1;
const EXPOSE: i8 = 2;

#[test]
fn the_request_replaces_an_exposed_mode_with_the_disguised_one() {
    // SAFETY: both calls only read or set this test process's own mode.
    let before = unsafe { RtlSetProcessPlaceholderCompatibilityMode(EXPOSE) };
    assert!(before >= 0, "set failed: {before}");
    assert_eq!(
        unsafe { RtlQueryProcessPlaceholderCompatibilityMode() },
        EXPOSE
    );

    assert_eq!(disguise_placeholders(), Ok(()));
    assert_eq!(
        unsafe { RtlQueryProcessPlaceholderCompatibilityMode() },
        DISGUISE
    );
    // Repeating the request keeps the mode.
    assert_eq!(disguise_placeholders(), Ok(()));
    assert_eq!(
        unsafe { RtlQueryProcessPlaceholderCompatibilityMode() },
        DISGUISE
    );
}
