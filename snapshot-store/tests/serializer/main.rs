mod support;

use du_analysis_index::ResultVersion;
use du_core_contracts::{
    Timestamp,
    result::{ResultCompleteness, ResultVersionRead},
};
use du_snapshot_store::{
    envelope, records,
    serializer::{SnapshotIdentity, SourceMetadata, write_v1},
};
use serde_json::{Value, json};
use std::io::{self, Write};
use support::{
    Decoded, decoded, empty_result, encoding, identity, native_path, root_placeholder,
    snapshot_size, source_metadata, source_result, validate_sealed,
};

fn write_snapshot(result: &ResultVersion, source: &SourceMetadata) -> Result<Vec<u8>, String> {
    let (snapshot_id, created_at) = identity()?;
    let mut bytes = Vec::new();
    write_v1(
        result,
        SnapshotIdentity {
            snapshot_id: &snapshot_id,
            created_at: &created_at,
            app_version: "0.0.1",
        },
        source,
        &mut bytes,
    )
    .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn assert_known_metadata(
    result: &ResultVersion,
    decoded: &Decoded,
    snapshot_id: &str,
    created_at: &str,
) {
    let metadata = &decoded.metadata;
    assert_eq!(metadata["source"]["completeness"], "missing");
    assert_eq!(
        metadata["source"]["resultVersionId"],
        result.version_id().as_str()
    );
    assert_eq!(
        metadata["source"]["scopeVersionId"],
        result.scope_version_id().as_str()
    );
    let observed_at = result.observed_at().map(ToString::to_string);
    assert_eq!(
        metadata["source"]["observedAt"].as_str(),
        observed_at.as_deref()
    );
    assert_eq!(metadata["source"]["conditionSummary"]["state"], "known");
    assert_eq!(
        metadata["source"]["conditionSummary"]["entries"][0],
        json!({"filter":"included"})
    );
    assert_eq!(
        metadata["source"]["algorithms"]["entries"][0]["id"],
        "size.subtree"
    );
    assert_eq!(
        metadata["source"]["pathRules"][0]["caseSensitivity"],
        "sensitive"
    );
    assert_eq!(metadata["source"]["pathRules"][2]["normalization"], "none");
    assert_eq!(metadata["snapshotId"], snapshot_id);
    assert_eq!(metadata["createdAt"], created_at);
    assert_eq!(metadata["appVersion"], "0.0.1");
    let roots: Vec<_> = result
        .observed_roots()
        .iter()
        .map(|root| Value::String(root.path.raw.as_str().to_owned()))
        .collect();
    assert_eq!(metadata["source"]["scopeRoots"], json!(roots));
}

fn assert_volume_records(frames: &[(u8, Vec<u8>)]) -> Result<(), String> {
    let volume = records::volume::decode(&frames[0].1, 3).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        volume
            .roots
            .iter()
            .map(|root| root.volume_index)
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1), None]
    );
    assert_eq!(
        volume
            .roots
            .iter()
            .map(|root| root.item_number)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2), Some(3)]
    );
    assert_eq!(
        volume
            .volumes
            .iter()
            .map(|volume| volume.id)
            .collect::<Vec<_>>(),
        vec![&[1][..], &[2][..]]
    );
    assert!(matches!(
        volume.volumes[0].start,
        records::volume::Observation::Success {
            total: 100,
            used: 40,
            free: 60,
            ..
        }
    ));
    assert!(matches!(
        volume.volumes[1].end,
        records::volume::Observation::Failure {
            kind: "permission_denied",
            reason: "space.volume_denied",
            os_code: Some(5),
            ..
        }
    ));
    Ok(())
}

fn assert_item_records(result: &ResultVersion, frames: &[(u8, Vec<u8>)]) -> Result<(), String> {
    let sources = result.items();
    let serialized: Vec<_> = frames.iter().filter(|record| record.0 == 2).collect();
    assert_eq!(serialized.len(), sources.len());
    for (source, frame) in sources.iter().zip(&serialized) {
        let item =
            records::item::decode(&frame.1, encoding()).map_err(|error| format!("{error:?}"))?;
        assert_eq!(item.number, source.item_no + 1);
        assert_eq!(item.parent, source.parent.map_or(0, |parent| parent + 1));
        if source.parent.is_none() {
            assert_eq!(item.name, root_placeholder());
            let index = usize::try_from(source.item_no).map_err(|error| error.to_string())?;
            assert_eq!(item.identity, result.observed_roots()[index].file_id);
        } else {
            assert_eq!(item.name, source.name);
        }
        let aggregate = result
            .space(source.item_no)
            .ok_or("missing item aggregate")?;
        assert_eq!(
            item.sizes,
            [
                snapshot_size(&aggregate.logical.amount),
                snapshot_size(&aggregate.allocated.amount),
                snapshot_size(&aggregate.unique_physical.amount),
            ]
        );
    }
    let raw_name = if cfg!(windows) {
        "𝄞"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        vec![0xff, 0xfe]
    };
    let mut found = false;
    for frame in serialized {
        let item =
            records::item::decode(&frame.1, encoding()).map_err(|error| format!("{error:?}"))?;
        if item.name == raw_name {
            found = true;
            break;
        }
    }
    assert!(found, "raw native name was not preserved");
    Ok(())
}

fn assert_gap_records(result: &ResultVersion, frames: &[(u8, Vec<u8>)]) -> Result<(), String> {
    let gaps: Vec<_> = frames.iter().filter(|record| record.0 == 3).collect();
    assert_eq!(gaps.len(), result.scope_gaps().len());
    let gap =
        records::gap::decode(&gaps[0].1, encoding(), 3).map_err(|error| format!("{error:?}"))?;
    let source = &result.scope_gaps()[0];
    assert_eq!(gap.root_index, source.root_index);
    assert_eq!(gap.reason_key, source.reason_key);
    assert_eq!(
        gap.observed_at_ms,
        source.observed_at.as_ref().map(Timestamp::unix_millis)
    );
    assert_eq!(
        gap.path,
        source.path.iter().map(Vec::as_slice).collect::<Vec<_>>()
    );
    Ok(())
}

fn assert_sealed_roots(result: &ResultVersion, bytes: &[u8]) -> Result<(), String> {
    let roots = validate_sealed(bytes)?;
    let expected: Vec<Vec<u8>> = result
        .observed_roots()
        .iter()
        .map(|root| root.path.raw.bytes().map_err(|error| error.to_string()))
        .collect::<Result<_, _>>()?;
    assert_eq!(roots.roots(), expected);
    assert_eq!(roots.path_rules().map(|rules| rules.len()), Some(3));
    Ok(())
}

#[test]
fn empty_scan_round_trips_through_the_v1_reader() -> Result<(), String> {
    let result = empty_result()?;
    let bytes = write_snapshot(&result, &source_metadata(false, 1))?;
    let decoded = decoded(&bytes)?;
    assert_eq!(decoded.metadata["source"]["completeness"], "complete");
    assert_eq!(
        decoded.metadata["source"]["scopeRoots"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(decoded.records.len(), 3);

    let volume =
        records::volume::decode(&decoded.records[0].1, 1).map_err(|error| format!("{error:?}"))?;
    assert_eq!(volume.roots[0].volume_index, Some(0));
    assert_eq!(volume.roots[0].item_number, Some(1));
    let item = records::item::decode(&decoded.records[1].1, encoding())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        (item.number, item.parent, item.name),
        (1, 0, root_placeholder())
    );
    assert_eq!(item.identity, &[10]);
    assert_eq!(decoded.records[2].0, 0x7f);

    let roots = validate_sealed(&bytes)?;
    let expected = native_path(if cfg!(windows) { "C:\\empty" } else { "/empty" });
    assert_eq!(roots.roots(), &[expected]);
    Ok(())
}

#[test]
fn multi_root_stream_preserves_records_metadata_and_unknown_identity() -> Result<(), String> {
    let result = source_result(ResultCompleteness::Complete)?;
    assert_eq!(result.completeness(), ResultCompleteness::Missing);
    let (snapshot_id, created_at) = identity()?;
    let bytes = write_snapshot(&result, &source_metadata(true, 3))?;
    let decoded = decoded(&bytes)?;
    assert_known_metadata(
        &result,
        &decoded,
        snapshot_id.as_str(),
        &created_at.to_string(),
    );
    assert_eq!(decoded.records.first().map(|record| record.0), Some(1));
    assert_eq!(decoded.records.last().map(|record| record.0), Some(0x7f));
    assert_volume_records(&decoded.records)?;
    assert_item_records(&result, &decoded.records)?;
    assert_gap_records(&result, &decoded.records)?;
    assert_sealed_roots(&result, &bytes)?;
    Ok(())
}

#[test]
fn unknown_conditions_and_path_rules_encode_as_explicit_nulls() -> Result<(), String> {
    let result = source_result(ResultCompleteness::Partial)?;
    let bytes = write_snapshot(&result, &source_metadata(false, 3))?;
    let decoded = decoded(&bytes)?;
    assert_eq!(
        decoded.metadata["source"]["conditionSummary"],
        json!({"state":"unknown"})
    );
    assert_eq!(
        decoded.metadata["source"]["pathRules"][0]["caseSensitivity"],
        Value::Null
    );
    assert_eq!(
        decoded.metadata["source"]["pathRules"][0]["normalization"],
        Value::Null
    );
    assert_eq!(decoded.metadata["source"]["completeness"], "partial");
    Ok(())
}

#[test]
fn cancelled_result_is_rejected_without_touching_the_writer() -> Result<(), String> {
    let result = source_result(ResultCompleteness::Cancelled)?;
    let (snapshot_id, created_at) = identity()?;
    let mut bytes = Vec::new();
    let error = write_v1(
        &result,
        SnapshotIdentity {
            snapshot_id: &snapshot_id,
            created_at: &created_at,
            app_version: "0.0.1",
        },
        &source_metadata(false, 3),
        &mut bytes,
    )
    .err()
    .ok_or("cancelled result unexpectedly serialized")?;
    assert!(matches!(
        error,
        du_snapshot_store::serializer::Error::Cancelled
    ));
    assert!(bytes.is_empty());
    Ok(())
}

#[test]
fn accepts_a_short_writing_output() -> Result<(), String> {
    struct ShortWriter(Vec<u8>);
    impl Write for ShortWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let count = bytes.len().min(7);
            self.0.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let result = empty_result()?;
    let (snapshot_id, created_at) = identity()?;
    let mut output = ShortWriter(Vec::new());
    write_v1(
        &result,
        SnapshotIdentity {
            snapshot_id: &snapshot_id,
            created_at: &created_at,
            app_version: "0.0.1",
        },
        &source_metadata(false, 1),
        &mut output,
    )
    .map_err(|error| error.to_string())?;
    envelope::verify_v1(&output.0).map_err(|error| format!("{error:?}"))?;
    Ok(())
}
