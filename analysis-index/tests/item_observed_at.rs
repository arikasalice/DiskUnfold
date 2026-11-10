use du_analysis_index::IngestSink;
use du_core_contracts::{Id, Timestamp, result::ResultCompleteness};
fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
#[test]
fn item_time_is_optional_and_frozen_per_publication() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let first = build
        .publish_partial(id(2)?)
        .map_err(|e| format!("{e:?}"))?;
    let observed = Timestamp::from_unix_millis(10).ok_or("invalid time")?;
    build
        .set_item_observed_at(0, &[], observed.clone())
        .map_err(|e| format!("{e:?}"))?;
    let end = build
        .seal_at(
            id(3)?,
            ResultCompleteness::Complete,
            Timestamp::from_unix_millis(20),
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.item_observed_at(0), None);
    assert_eq!(end.item_observed_at(0), Some(&observed));
    assert_ne!(
        end.item_observed_at(0),
        Timestamp::from_unix_millis(20).as_ref()
    );
    Ok(())
}
