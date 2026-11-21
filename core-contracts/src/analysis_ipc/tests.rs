use super::{AnalysisTarget, IdentitySource, TargetAccess};
use crate::{OsPath, PathBytes, PathEncoding};

fn target() -> AnalysisTarget {
    AnalysisTarget {
        kind: None,
        path: OsPath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/"),
            display: "/".into(),
            lossy: false,
        },
        connected: true,
        volume_id: None,
        identity_source: IdentitySource::Unavailable,
        access: TargetAccess::Unknown,
        reason_key: None,
        default_visible: None,
        is_startup_volume: None,
        volume_name: None,
    }
}

#[test]
fn startup_fields_are_optional_and_reject_explicit_null() -> Result<(), serde_json::Error> {
    let mut value = serde_json::to_value(target())?;
    assert!(value.get("isStartupVolume").is_none());
    assert!(value.get("volumeName").is_none());

    value["isStartupVolume"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<AnalysisTarget>(value.clone()).is_err());
    let mut name = serde_json::to_value(target())?;
    name["volumeName"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<AnalysisTarget>(name).is_err());
    Ok(())
}
