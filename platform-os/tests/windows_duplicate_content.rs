#![cfg(windows)]

use du_core_contracts::{
    Id, ItemRef,
    duplicate_content::{ContentIdentity, DuplicateContentPort, PlaceholderEvidence},
    ports::PortErrorKind,
};
use du_platform_os::windows_duplicate::WindowsDuplicateContent;

#[test]
fn unverified_windows_guarantees_reject_all_placeholder_states_before_open() {
    let mut port = WindowsDuplicateContent;
    let identity = ContentIdentity {
        volume: vec![1],
        file: vec![2],
        enumerated_inode: Some(3),
    };
    for placeholder in [
        PlaceholderEvidence::Local,
        PlaceholderEvidence::DownloadRequired,
        PlaceholderEvidence::Unknown,
    ] {
        let item = ItemRef {
            result_version: Id::from_v7_bytes([1; 16]),
            item_no: 1,
        };
        let result = port.open_bound(&item, &identity, placeholder);
        assert!(
            matches!(result, Err(error) if error.kind == PortErrorKind::Unsupported && error.reason_key == "os.content_safety_unverified")
        );
    }
}
