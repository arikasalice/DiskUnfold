use du_analysis_index::{IngestSink, Observation};
use du_core_contracts::{
    Id, Timestamp, result::ResultCompleteness, scan_observation::NativeModifiedTime,
};
use std::num::NonZeroU64;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn native(n: u64) -> Result<NativeModifiedTime, String> {
    NonZeroU64::new(n)
        .map(NativeModifiedTime::WindowsFiletime)
        .ok_or("zero".into())
}
#[test]
fn native_evidence_is_frozen_per_version_and_cleared_on_display_replacement() -> Result<(), String>
{
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    assert!(
        build
            .set_native_modified_at(0, &[], native(10_001)?)
            .is_err()
    );
    assert!(
        build
            .set_native_modified_at(0, &[b"missing".to_vec()], native(10_001)?)
            .is_err()
    );
    let path = vec![b"file".to_vec()];
    build
        .add_file(Observation {
            path: path.clone(),
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let display = Timestamp::from_unix_millis(10).ok_or("invalid")?;
    build
        .set_modified_at(0, &path, display.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_native_modified_at(0, &path, native(10_001)?)
        .map_err(|e| format!("{e:?}"))?;
    let first = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.item_native_modified_at(0), None);
    assert_eq!(first.item_native_modified_at(1), Some(native(10_001)?));
    assert_eq!(first.item_native_modified_at(u32::MAX), None);
    build
        .set_modified_at(0, &path, display)
        .map_err(|e| format!("{e:?}"))?;
    let next = build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(next.item_native_modified_at(1), None);
    assert_eq!(first.item_native_modified_at(1), Some(native(10_001)?));
    Ok(())
}
