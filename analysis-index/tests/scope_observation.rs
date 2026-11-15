//! The meaning of a scan-completion observation and the sealed-result lookup behind it.
use du_analysis_index::{
    IngestSink, ObservationSkip, ObservationStamp, SampleSessionKey, ScopeHeader, SealedScanError,
    fingerprint_from_hex,
};
use du_core_contracts::{
    Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue, Timestamp,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
    scope_observation_log::{
        ObservedAmount, ObservedSize, SCOPE_FINGERPRINT_FORMAT_V1, ScopeObservationOrigin,
    },
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const HEX: &str = "00ff10abcdef0123456789abcdef0123456789abcdef0123456789abcdef9a5c";

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn root() -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&[67, 0]),
        },
        volume_id: vec![1],
        file_id: vec![1],
    }
}

fn stamp() -> Result<ObservationStamp, String> {
    Ok(ObservationStamp {
        observation_id: id(900)?,
        job_id: id(901)?,
        created_at_ms: 77,
    })
}

fn header(fingerprint: &str, version: u32) -> Result<ScopeHeader, String> {
    Ok(ScopeHeader {
        version_id: id(version)?,
        fingerprint: fingerprint.into(),
    })
}

fn sealed(
    sink: &IngestSink,
    header: ScopeHeader,
    seal: u32,
    completeness: ResultCompleteness,
    observed_at: Option<i64>,
) -> Result<std::sync::Arc<du_analysis_index::ResultVersion>, String> {
    let mut build = sink
        .open_scoped_forest(header, vec![root()], None)
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal_at(
            id(seal)?,
            completeness,
            observed_at.and_then(Timestamp::from_unix_millis),
        )
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn a_sealed_result_becomes_one_row_of_its_scope_totals() -> TestResult {
    let sink = IngestSink::default();
    let version = sealed(
        &sink,
        header(HEX, 1)?,
        2,
        ResultCompleteness::Complete,
        Some(1_234),
    )?;
    let row = version
        .scope_observation(stamp()?)
        .map_err(|e| format!("{e:?}"))?;
    let total = version.scope_total();
    assert_eq!(row.observation_id, id(900)?);
    assert_eq!(row.job_id, id(901)?);
    assert_eq!(row.created_at_ms, 77);
    assert_eq!(row.observed_at_ms, 1_234);
    assert_eq!(row.fingerprint_format, SCOPE_FINGERPRINT_FORMAT_V1);
    assert_eq!(
        row.scope_fingerprint,
        fingerprint_from_hex(HEX).ok_or("hex")?
    );
    assert_eq!(row.origin, ScopeObservationOrigin::Scan);
    assert_eq!(row.completeness, ResultCompleteness::Complete);
    assert_eq!(
        (row.size_rule_id.as_str(), row.size_rule_revision),
        (version.size_rule().id, version.size_rule().revision)
    );
    for (stored, source, basis) in [
        (&row.logical, &total.logical, SizeBasis::Logical),
        (&row.allocated, &total.allocated, SizeBasis::Allocated),
        (
            &row.unique_physical,
            &total.unique_physical,
            SizeBasis::UniquePhysical,
        ),
    ] {
        assert_eq!(source.basis, basis);
        assert_eq!(stored.evidence, source.evidence);
        let expected = match &source.amount {
            SizeAmount::Exact { bytes } => ObservedAmount::Exact(bytes.get()),
            SizeAmount::LowerBound { bytes } => ObservedAmount::LowerBound(bytes.get()),
            SizeAmount::Unknown => ObservedAmount::Unknown,
        };
        assert_eq!(stored.amount, expected);
    }
    Ok(())
}

#[test]
fn a_partial_result_keeps_its_completeness_and_its_states() -> TestResult {
    let sink = IngestSink::default();
    let version = sealed(
        &sink,
        header(HEX, 1)?,
        2,
        ResultCompleteness::Partial,
        Some(5),
    )?;
    let row = version
        .scope_observation(stamp()?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(row.completeness, ResultCompleteness::Partial);
    let total = version.scope_total();
    let unchanged = |stored: &ObservedSize, source: &SizeValue| {
        matches!(
            (&stored.amount, &source.amount),
            (ObservedAmount::Exact(_), SizeAmount::Exact { .. })
                | (ObservedAmount::LowerBound(_), SizeAmount::LowerBound { .. })
                | (ObservedAmount::Unknown, SizeAmount::Unknown)
        ) && stored.evidence == source.evidence
    };
    assert!(unchanged(&row.logical, &total.logical));
    assert!(unchanged(&row.allocated, &total.allocated));
    assert!(unchanged(&row.unique_physical, &total.unique_physical));
    // An empty scan has no measured data, so these totals are not claimed to be exact.
    assert!(matches!(
        row.unique_physical.amount,
        ObservedAmount::Unknown | ObservedAmount::LowerBound(_)
    ));
    assert_ne!(row.unique_physical.evidence, SizeEvidence::Measured);
    Ok(())
}

#[test]
fn a_result_without_an_observation_time_yields_no_row() -> TestResult {
    let sink = IngestSink::default();
    let version = sealed(
        &sink,
        header(HEX, 1)?,
        2,
        ResultCompleteness::Complete,
        None,
    )?;
    assert_eq!(
        version.scope_observation(stamp()?).err(),
        Some(ObservationSkip::NoObservedAt)
    );
    Ok(())
}

#[test]
fn a_missing_or_malformed_fingerprint_is_never_repaired() -> TestResult {
    let sink = IngestSink::default();
    let mut build = sink
        .open_forest(id(1)?, vec![root()])
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let unscoped = build
        .seal_at(
            id(2)?,
            ResultCompleteness::Complete,
            Timestamp::from_unix_millis(1),
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        unscoped.scope_observation(stamp()?).err(),
        Some(ObservationSkip::NoFingerprint)
    );
    let upper = HEX.to_uppercase();
    let short = &HEX[..62];
    let long = format!("{HEX}0");
    let non_hex = format!("{}g", &HEX[..63]);
    for (n, bad) in [
        upper.as_str(),
        short,
        long.as_str(),
        non_hex.as_str(),
        "scope-fingerprint",
    ]
    .into_iter()
    .enumerate()
    {
        let sink = IngestSink::default();
        let version = sealed(
            &sink,
            header(bad, 10)?,
            11,
            ResultCompleteness::Complete,
            Some(1 + n as i64),
        )?;
        assert_eq!(
            version.scope_observation(stamp()?).err(),
            Some(ObservationSkip::MalformedFingerprint),
            "{bad}"
        );
        assert_eq!(fingerprint_from_hex(bad), None);
    }
    Ok(())
}

#[test]
fn hex_conversion_covers_every_digit_and_byte_order() {
    assert_eq!(fingerprint_from_hex(&"0".repeat(64)), Some([0; 32]));
    assert_eq!(fingerprint_from_hex(&"f".repeat(64)), Some([255; 32]));
    let mut expected = [0u8; 32];
    expected[0] = 0x01;
    expected[1] = 0x23;
    expected[31] = 0xab;
    let text = format!("0123{}ab", "0".repeat(58));
    assert_eq!(fingerprint_from_hex(&text), Some(expected));
}

#[test]
fn a_sample_result_yields_no_row() -> TestResult {
    let sink = IngestSink::default();
    let mut build = sink
        .open_sample_forest(id(1)?, vec![root()], &SampleSessionKey::new(id(2)?))
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal_at(
            id(3)?,
            ResultCompleteness::Complete,
            Timestamp::from_unix_millis(1),
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        version.scope_observation(stamp()?).err(),
        Some(ObservationSkip::NotRealSource)
    );
    Ok(())
}

#[test]
fn only_a_sealed_and_still_current_result_is_returned_to_the_recorder() -> TestResult {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis(HEX, id(1)?)
        .map_err(|e| format!("{e:?}"))?;
    let mut first = sink
        .open_scoped_forest(header(HEX, 2)?, vec![root()], Some(lineage.clone()))
        .map_err(|e| format!("{e:?}"))?;
    first
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let interim = first
        .publish_partial(id(3)?)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        sink.sealed_scan_result(interim.version_id()).err(),
        Some(SealedScanError::NotSealed)
    );
    let final_one = first
        .seal_at(
            id(4)?,
            ResultCompleteness::Complete,
            Timestamp::from_unix_millis(10),
        )
        .map_err(|e| format!("{e:?}"))?;
    let found = sink
        .sealed_scan_result(final_one.version_id())
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(found.version_id(), final_one.version_id());
    // An unknown identifier is not current.
    assert_eq!(
        sink.sealed_scan_result(&id(99)?).err(),
        Some(SealedScanError::NotCurrent)
    );
    // A withdrawn candidate is not current once its build aborts.
    let mut second = sink
        .open_scoped_forest(header(HEX, 5)?, vec![root()], Some(lineage.clone()))
        .map_err(|e| format!("{e:?}"))?;
    second
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let candidate = second
        .publish_partial(id(6)?)
        .map_err(|e| format!("{e:?}"))?;
    second.abort();
    assert_eq!(
        sink.sealed_scan_result(candidate.version_id()).err(),
        Some(SealedScanError::NotCurrent)
    );
    assert!(sink.sealed_scan_result(final_one.version_id()).is_ok());
    // A newer sealed result replaces the older one in the same lineage.
    let mut third = sink
        .open_scoped_forest(header(HEX, 7)?, vec![root()], Some(lineage))
        .map_err(|e| format!("{e:?}"))?;
    third
        .add_directory_at(0, Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    let newest = third
        .seal_at(
            id(8)?,
            ResultCompleteness::Complete,
            Timestamp::from_unix_millis(20),
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        sink.sealed_scan_result(final_one.version_id()).err(),
        Some(SealedScanError::NotCurrent)
    );
    assert!(sink.sealed_scan_result(newest.version_id()).is_ok());
    Ok(())
}
