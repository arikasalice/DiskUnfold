use super::*;

#[test]
fn sealed_gap_validation_uses_root_count_and_discards_invalid_stage() -> Result<(), String> {
    use crate::records::{NameEncoding, gap::Gap};
    let gap = Gap {
        root_index: 1,
        path: vec![b"missing"],
        reason_key: "scan.denied",
        observed_at_ms: None,
    };
    let body = gap
        .encode(NameEncoding::Bytes, 2)
        .map_err(|e| format!("{e:?}"))?;
    let make = |body: &[u8]| {
        let mut records = vec![1, 0, 0, 0, 1];
        records.extend_from_slice(&((body.len() + 1) as u32).to_le_bytes());
        records.push(3);
        records.extend_from_slice(body);
        records.extend_from_slice(&[1, 0, 0, 0, 127]);
        records
    };
    let records = make(&body);
    let compressed = frame(&records, true).map_err(|e| e.to_string())?;
    let mut spool = sealed(&envelope_with_count(&compressed, records.len() as u64, 3))?;
    let stage = validate_items_and_gaps_in_sealed(
        &mut spool,
        Stage::default(),
        NameEncoding::Bytes,
        2,
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(stage.bytes, records);
    let mut invalid = body.clone();
    invalid.push(0);
    for (bytes, count) in [
        (&body[..], 1),
        (&invalid[..], 2),
        (&body[..body.len() - 1], 2),
    ] {
        let records = make(bytes);
        let compressed = frame(&records, true).map_err(|e| e.to_string())?;
        let mut spool = sealed(&envelope_with_count(&compressed, records.len() as u64, 3))?;
        let stage = Stage::default();
        let discarded = stage.discarded.clone();
        assert!(matches!(
            validate_items_and_gaps_in_sealed(
                &mut spool,
                stage,
                NameEncoding::Bytes,
                count,
                || false
            ),
            Err(Error::Records)
        ));
        assert_eq!(discarded.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    Ok(())
}
