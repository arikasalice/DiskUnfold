use du_platform_os::startup::{
    FullDiskAccessStatus, PrivacySettingsOpenOutcome, StartupCapabilitiesPort,
    StartupDialogOutcome, SystemStartupCapabilities, WebViewAvailability,
};

#[test]
fn startup_full_disk_access_observation_is_conservative() {
    let system = SystemStartupCapabilities;

    #[cfg(target_os = "macos")]
    assert_eq!(
        system.full_disk_access_status(),
        FullDiskAccessStatus::Unknown
    );

    #[cfg(not(target_os = "macos"))]
    assert_eq!(
        system.full_disk_access_status(),
        FullDiskAccessStatus::NotApplicable
    );
}

#[test]
fn startup_webview_preflight_does_not_infer_runtime_usability() {
    let system = SystemStartupCapabilities;

    #[cfg(feature = "webview")]
    assert_eq!(system.webview_availability(), WebViewAvailability::Unknown);

    #[cfg(not(feature = "webview"))]
    assert_eq!(
        system.webview_availability(),
        WebViewAvailability::Unavailable
    );
}

#[cfg(feature = "fake")]
#[test]
fn fake_startup_capabilities_are_deterministic_and_do_not_open_ui() {
    use du_platform_os::startup::FakeStartupCapabilities;

    let fake = FakeStartupCapabilities::new(
        FullDiskAccessStatus::Denied,
        WebViewAvailability::Available,
        PrivacySettingsOpenOutcome::RequestAccepted,
        StartupDialogOutcome::Acknowledged,
    );
    assert_eq!(fake.full_disk_access_status(), FullDiskAccessStatus::Denied);
    assert_eq!(fake.webview_availability(), WebViewAvailability::Available);
    assert_eq!(fake.privacy_settings_requests(), 0);
    assert_eq!(fake.startup_dialog_requests(), 0);

    assert_eq!(
        fake.open_privacy_settings(),
        PrivacySettingsOpenOutcome::RequestAccepted
    );
    assert_eq!(fake.privacy_settings_requests(), 1);
    assert_eq!(
        fake.show_pre_window_error_dialog("Startup blocked", "A required service is unavailable."),
        StartupDialogOutcome::Acknowledged
    );
    assert_eq!(fake.startup_dialog_requests(), 1);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn privacy_settings_action_is_not_applicable_off_macos() {
    let system = SystemStartupCapabilities;
    assert_eq!(
        system.open_privacy_settings(),
        PrivacySettingsOpenOutcome::NotApplicable
    );
}

#[cfg(all(target_os = "macos", not(feature = "webview")))]
#[test]
fn privacy_settings_action_is_unsupported_without_macos_adapter() {
    let system = SystemStartupCapabilities;
    assert_eq!(
        system.open_privacy_settings(),
        PrivacySettingsOpenOutcome::Unsupported
    );
}
