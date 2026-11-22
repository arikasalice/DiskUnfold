use super::*;

fn observation(identity: FileIdentity, sharing: SharingEvidence) -> SpaceObservation {
    SpaceObservation {
        allocated: Some(SizeValue {
            basis: SizeBasis::Allocated,
            evidence: SizeEvidence::OsReported,
            amount: SizeAmount::Exact {
                bytes: 17_u64.into(),
            },
        }),
        identity: Some(identity),
        link_count: Some(2),
        sharing,
    }
}

#[test]
fn compact_column_round_trips_inline_and_arena_identities() -> Result<(), String> {
    let inline = observation(
        FileIdentity {
            volume: (0..8).collect(),
            file: (8..16).collect(),
        },
        SharingEvidence::Unknown,
    );
    let arena = observation(
        FileIdentity {
            volume: vec![1, 2, 3],
            file: vec![4; 12],
        },
        SharingEvidence::None,
    );
    let empty_identity = observation(
        FileIdentity {
            volume: Vec::new(),
            file: Vec::new(),
        },
        SharingEvidence::Unknown,
    );
    let values = vec![
        None,
        Some(Arc::new(inline.clone())),
        None,
        Some(Arc::new(arena.clone())),
        Some(Arc::new(empty_identity.clone())),
    ];
    let compact = compact_test_observations(Some(&values))
        .ok_or_else(|| "failed to compact sparse observations".to_owned())?;
    assert!(matches!(&compact.records, Records::Sparse { .. }));
    for (item_no, expected) in values.iter().enumerate() {
        assert_eq!(
            compact
                .get(u32::try_from(item_no).map_err(|error| format!("{error}"))?)
                .map(PhysicalObservationRef::to_owned),
            expected.as_deref().cloned()
        );
    }

    let mut builder = PhysicalObservations::builder(2, 2);
    for item_no in 0..2 {
        builder
            .push(item_no, Some(&inline))
            .map_err(|error| format!("{error:?}"))?;
    }
    let dense = builder.finish();
    assert!(matches!(&dense.records, Records::Dense(_)));
    assert_eq!(
        dense.get(1).map(PhysicalObservationRef::to_owned),
        Some(inline)
    );

    let volume = b"volume-identifier";
    let mut builder = PhysicalObservations::builder(2, 2);
    for file_number in 0..2_u64 {
        let shared_volume = observation(
            FileIdentity {
                volume: volume.to_vec(),
                file: file_number.to_le_bytes().to_vec(),
            },
            SharingEvidence::None,
        );
        builder
            .push(file_number as u32, Some(&shared_volume))
            .map_err(|error| format!("{error:?}"))?;
    }
    let interned = builder.finish();
    assert_eq!(interned.arena, volume);
    assert_eq!(
        interned
            .get(0)
            .map(PhysicalObservationRef::to_owned)
            .and_then(|value| value.identity),
        Some(FileIdentity {
            volume: volume.to_vec(),
            file: 0_u64.to_le_bytes().to_vec(),
        })
    );
    assert!(
        matches!(&interned.records, Records::Dense(records) if records.iter().all(|r| r.flags & INLINE_FILE_IDENTITY != 0))
    );
    Ok(())
}
