use super::*;

#[test]
fn explicit_device_disconnection_stays_distinct_from_other_io() {
    let disconnected = error(1167);
    assert_eq!(disconnected.kind, PortErrorKind::Io);
    assert_eq!(disconnected.reason_key, "scan.disconnected");
    assert_eq!(disconnected.os_code, Some(1167));
    assert_eq!(error(1117).reason_key, "os.query_failed");
    assert_eq!(error(5).kind, PortErrorKind::PermissionDenied);
}

#[test]
fn only_a_win32_hresult_interprets_the_low_word_as_a_system_error() {
    let win32 = win_error(windows::core::Error::from_hresult(HRESULT::from_win32(
        1167,
    )));
    assert_eq!(win32.reason_key, "scan.disconnected");

    let other_facility = HRESULT(0x8004_048f_u32 as i32);
    let unrelated = win_error(windows::core::Error::from_hresult(other_facility));
    assert_eq!(unrelated.reason_key, "os.query_failed");
    assert_eq!(unrelated.os_code, Some(other_facility.0));
}
