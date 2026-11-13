use du_core_contracts::startup::{
    FullDiskAccessStatus, PrivacySettingsOpenOutcome, StartupCapabilitiesPort,
    StartupDialogOutcome, WebViewAvailability,
};
use du_platform_os::startup::{
    FullDiskAccessStatus as PlatformFullDiskAccessStatus,
    PrivacySettingsOpenOutcome as PlatformPrivacySettingsOpenOutcome,
    StartupCapabilitiesPort as PlatformStartupCapabilitiesPort,
    StartupDialogOutcome as PlatformStartupDialogOutcome, SystemStartupCapabilities,
    WebViewAvailability as PlatformWebViewAvailability,
};

#[test]
fn platform_os_keeps_the_shared_startup_contract_paths() {
    fn assert_core_contract<T: StartupCapabilitiesPort>() {}
    fn assert_platform_path<T: PlatformStartupCapabilitiesPort>() {}

    assert_core_contract::<SystemStartupCapabilities>();
    assert_platform_path::<SystemStartupCapabilities>();

    let _: PlatformFullDiskAccessStatus = FullDiskAccessStatus::Unknown;
    let _: PlatformWebViewAvailability = WebViewAvailability::Unknown;
    let _: PlatformPrivacySettingsOpenOutcome = PrivacySettingsOpenOutcome::Unsupported;
    let _: PlatformStartupDialogOutcome = StartupDialogOutcome::Unsupported;
}

#[test]
fn startup_full_disk_access_observation_is_conservative() {
    let system = SystemStartupCapabilities;

    // The real answer depends on the host's grant; it is never NotApplicable.
    #[cfg(target_os = "macos")]
    assert_ne!(
        system.full_disk_access_status(),
        FullDiskAccessStatus::NotApplicable
    );

    #[cfg(not(target_os = "macos"))]
    assert_eq!(
        system.full_disk_access_status(),
        FullDiskAccessStatus::NotApplicable
    );
}

#[test]
fn startup_webview_preflight_reports_only_observed_runtime_evidence() {
    let system = SystemStartupCapabilities;

    // Windows reads the registered WebView2 runtime. Linux and macOS read the
    // version their linked WebKit reports, which a running test binary proves is
    // loaded. Any other platform keeps a compiled adapter from implying a session.
    #[cfg(all(feature = "webview", windows))]
    assert_ne!(
        system.webview_availability(),
        WebViewAvailability::Unknown,
        "a host that can run this test either registers or lacks the runtime"
    );

    #[cfg(all(feature = "webview", any(target_os = "linux", target_os = "macos")))]
    assert_eq!(
        system.webview_availability(),
        WebViewAvailability::Available,
        "the linked WebKit reports its own version"
    );

    #[cfg(all(
        feature = "webview",
        not(any(windows, target_os = "linux", target_os = "macos"))
    ))]
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
        du_core_contracts::startup::PrivacySettingsOpenOutcome::NotApplicable
    );
}

#[cfg(all(target_os = "macos", not(feature = "webview")))]
#[test]
fn privacy_settings_action_is_unsupported_without_macos_adapter() {
    let system = SystemStartupCapabilities;
    assert_eq!(
        system.open_privacy_settings(),
        du_core_contracts::startup::PrivacySettingsOpenOutcome::Unsupported
    );
}
