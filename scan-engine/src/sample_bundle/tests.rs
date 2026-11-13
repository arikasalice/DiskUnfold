use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use super::{
    BundleDecodeLimits, BundleFailure, BundleFormatReader, BundleFormatReaderRegistry,
    BundleHeader, BundleSourcePort, DecodedBundleRecords, DecodedObservation, FormatReaderError,
    MAX_BUNDLE_ITEMS, MAX_BUNDLE_SIZE_BYTES, MAX_OBSERVATIONS, RecordScenarioReference,
    ValidatedBundle, validate_bundle, validate_with_digest,
};

#[derive(Clone)]
struct FixtureSource {
    body: Option<Arc<[u8]>>,
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl BundleSourcePort for FixtureSource {
    fn read_bundle(&self) -> Option<Arc<[u8]>> {
        self.trace.borrow_mut().push("presence");
        self.body.clone()
    }
}

struct FixtureReader {
    body: Arc<[u8]>,
    expected_payload: Vec<u8>,
    header: BundleHeader,
    decoded: Result<DecodedBundleRecords, FormatReaderError>,
    trace: Rc<RefCell<Vec<&'static str>>>,
    record_payload_address: Rc<Cell<Option<usize>>>,
}

impl BundleFormatReader for FixtureReader {
    fn read_header(&self, body: &Arc<[u8]>) -> Result<BundleHeader, FormatReaderError> {
        self.trace.borrow_mut().push("header");
        if !Arc::ptr_eq(body, &self.body) {
            return Err(FormatReaderError);
        }
        Ok(self.header.clone())
    }

    fn decode_records(
        &self,
        payload: &[u8],
        _header: &BundleHeader,
        limits: BundleDecodeLimits,
    ) -> Result<DecodedBundleRecords, FormatReaderError> {
        self.trace.borrow_mut().push("records");
        self.record_payload_address
            .set(Some(payload.as_ptr() as usize));
        if payload != self.expected_payload.as_slice()
            || limits
                != (BundleDecodeLimits {
                    max_items: MAX_BUNDLE_ITEMS,
                    max_observations: MAX_OBSERVATIONS,
                })
        {
            return Err(FormatReaderError);
        }
        self.decoded.clone()
    }
}

struct FixtureRegistry {
    reader: FixtureReader,
    supported: bool,
}

impl BundleFormatReaderRegistry for FixtureRegistry {
    fn reader_for<'a>(&'a self, body: &Arc<[u8]>) -> Option<&'a dyn BundleFormatReader> {
        self.reader.trace.borrow_mut().push("reader");
        (self.supported && Arc::ptr_eq(body, &self.reader.body))
            .then_some(&self.reader as &dyn BundleFormatReader)
    }
}

struct Fixture {
    source: FixtureSource,
    registry: FixtureRegistry,
    trace: Rc<RefCell<Vec<&'static str>>>,
    record_payload_address: Rc<Cell<Option<usize>>>,
}

impl Fixture {
    fn new(body: Vec<u8>, item_count: u64, observations: usize) -> Self {
        let body: Arc<[u8]> = Arc::from(body);
        let trace = Rc::new(RefCell::new(Vec::new()));
        let header = BundleHeader {
            bundle_version: "sample-1".to_owned(),
            digest_algorithm: "blake3".to_owned(),
            digest: *blake3::hash(&body).as_bytes(),
            payload_range: 0..body.len(),
            declared_item_count: item_count,
            scenario_ids: vec!["tree".to_owned()],
        };
        let record_payload_address = Rc::new(Cell::new(None));
        let reader = FixtureReader {
            body: Arc::clone(&body),
            expected_payload: body.to_vec(),
            header,
            decoded: Ok(DecodedBundleRecords {
                scenario_references: vec![RecordScenarioReference {
                    scenario_id: "tree".to_owned(),
                }],
                observations: vec![DecodedObservation; observations],
            }),
            trace: Rc::clone(&trace),
            record_payload_address: Rc::clone(&record_payload_address),
        };
        Self {
            source: FixtureSource {
                body: Some(body),
                trace: Rc::clone(&trace),
            },
            registry: FixtureRegistry {
                reader,
                supported: true,
            },
            trace,
            record_payload_address,
        }
    }

    fn decoded_mut(&mut self) -> &mut Result<DecodedBundleRecords, FormatReaderError> {
        &mut self.registry.reader.decoded
    }

    fn header_mut(&mut self) -> &mut BundleHeader {
        &mut self.registry.reader.header
    }
}

fn verify(fixture: &Fixture) -> Result<ValidatedBundle, BundleFailure> {
    validate_bundle(&fixture.source, &fixture.registry)
}

#[test]
fn missing_bundle_stops_before_format_lookup() {
    let mut fixture = Fixture::new(vec![1], 1, 1);
    fixture.source.body = None;

    assert_eq!(verify(&fixture), Err(BundleFailure::Missing));
    assert_eq!(*fixture.trace.borrow(), vec!["presence"]);
}

#[test]
fn unsupported_reader_wins_over_oversize_body() {
    let mut fixture = Fixture::new(vec![0; MAX_BUNDLE_SIZE_BYTES + 1], 1, 1);
    fixture.registry.supported = false;

    assert_eq!(verify(&fixture), Err(BundleFailure::UnsupportedFormat));
    assert_eq!(*fixture.trace.borrow(), vec!["presence", "reader"]);
}

#[test]
fn digest_is_checked_before_size_and_count_limits() {
    let mut fixture = Fixture::new(vec![0; MAX_BUNDLE_SIZE_BYTES + 1], 1, 0);
    fixture.registry.reader.header.declared_item_count = (MAX_BUNDLE_ITEMS + 1) as u64;
    fixture.registry.reader.header.digest[0] ^= 0xff;
    let mut events = Vec::new();

    let result = validate_with_digest(&fixture.source, &fixture.registry, |body| {
        events.push("hash");
        *blake3::hash(body).as_bytes()
    });

    assert_eq!(result, Err(BundleFailure::Corrupt));
    assert_eq!(events, vec!["hash"]);
    assert_eq!(
        *fixture.trace.borrow(),
        vec!["presence", "reader", "header"]
    );
}

#[test]
fn exact_size_and_item_count_boundaries_are_accepted() {
    let body = vec![1; MAX_BUNDLE_SIZE_BYTES];
    let mut fixture = Fixture::new(body, MAX_BUNDLE_ITEMS as u64, 2);
    *fixture.decoded_mut() = Ok(DecodedBundleRecords {
        scenario_references: vec![
            RecordScenarioReference {
                scenario_id: "tree".to_owned(),
            };
            MAX_BUNDLE_ITEMS
        ],
        observations: vec![DecodedObservation; MAX_OBSERVATIONS],
    });

    let verified = verify(&fixture);
    assert!(verified.is_ok());
    if let Ok(view) = verified {
        assert_eq!(view.record_bytes().len(), MAX_BUNDLE_SIZE_BYTES);
        assert_eq!(view.record_references().len(), MAX_BUNDLE_ITEMS);
        assert_eq!(view.observation_count(), MAX_OBSERVATIONS);
    }
}

#[test]
fn size_overflow_stops_before_record_parsing() {
    let fixture = Fixture::new(vec![1; MAX_BUNDLE_SIZE_BYTES + 1], 1, 1);

    assert_eq!(verify(&fixture), Err(BundleFailure::Corrupt));
    assert_eq!(
        *fixture.trace.borrow(),
        vec!["presence", "reader", "header"]
    );
}

#[test]
fn declared_item_count_overflow_stops_before_record_parsing() {
    let fixture = Fixture::new(vec![1], (MAX_BUNDLE_ITEMS + 1) as u64, 1);

    assert_eq!(verify(&fixture), Err(BundleFailure::Corrupt));
    assert_eq!(
        *fixture.trace.borrow(),
        vec!["presence", "reader", "header"]
    );
}

#[test]
fn observation_count_is_derived_from_decoded_points() {
    for count in [0, 3] {
        let fixture = Fixture::new(vec![1], 1, count);
        assert_eq!(verify(&fixture), Err(BundleFailure::Corrupt));
        assert_eq!(
            *fixture.trace.borrow(),
            vec!["presence", "reader", "header", "records"]
        );
    }

    for count in [1, 2] {
        let fixture = Fixture::new(vec![1], 1, count);
        assert!(verify(&fixture).is_ok());
    }
}

#[test]
fn digest_covers_the_reader_selected_payload_not_header_bytes() {
    let mut fixture = Fixture::new(b"HEADERpayload".to_vec(), 1, 1);
    fixture.header_mut().payload_range = 6..13;
    fixture.header_mut().digest = *blake3::hash(b"payload").as_bytes();
    fixture.registry.reader.expected_payload = b"payload".to_vec();
    let mut digest_payload_address = None;

    let result = validate_with_digest(&fixture.source, &fixture.registry, |payload| {
        digest_payload_address = Some(payload.as_ptr() as usize);
        *blake3::hash(payload).as_bytes()
    });

    assert!(result.is_ok());
    assert_eq!(digest_payload_address, fixture.record_payload_address.get());
    if let Ok(view) = result {
        assert_eq!(view.record_bytes(), b"payload");
    }
}

#[test]
fn payload_tampering_fails_digest_verification() {
    let mut fixture = Fixture::new(b"HEADERpayload".to_vec(), 1, 1);
    fixture.header_mut().payload_range = 6..13;
    fixture.header_mut().digest = *blake3::hash(b"payload").as_bytes();
    let tampered: Arc<[u8]> = Arc::from(b"HEADERpayloax".to_vec());
    fixture.source.body = Some(Arc::clone(&tampered));
    fixture.registry.reader.body = tampered;

    assert_eq!(verify(&fixture), Err(BundleFailure::Corrupt));
    assert_eq!(
        *fixture.trace.borrow(),
        vec!["presence", "reader", "header"]
    );
}

#[test]
fn invalid_payload_range_is_rejected_before_hashing_or_record_parsing() {
    let mut fixture = Fixture::new(vec![1], 1, 1);
    fixture.header_mut().payload_range = 0..2;
    let mut events = Vec::new();

    let result = validate_with_digest(&fixture.source, &fixture.registry, |body| {
        events.push("hash");
        *blake3::hash(body).as_bytes()
    });

    assert_eq!(result, Err(BundleFailure::Corrupt));
    assert!(events.is_empty());
    assert_eq!(
        *fixture.trace.borrow(),
        vec!["presence", "reader", "header"]
    );
}

#[test]
fn header_declared_item_count_must_match_decoded_records() {
    let mut fixture = Fixture::new(vec![1], 2, 1);
    *fixture.decoded_mut() = Ok(DecodedBundleRecords {
        scenario_references: vec![RecordScenarioReference {
            scenario_id: "tree".to_owned(),
        }],
        observations: vec![DecodedObservation],
    });

    assert_eq!(verify(&fixture), Err(BundleFailure::Corrupt));
}

#[test]
fn invalid_scenario_reference_prevents_view_availability() {
    let mut fixture = Fixture::new(vec![1], 2, 2);
    *fixture.decoded_mut() = Ok(DecodedBundleRecords {
        scenario_references: vec![
            RecordScenarioReference {
                scenario_id: "tree".to_owned(),
            },
            RecordScenarioReference {
                scenario_id: "missing-scenario".to_owned(),
            },
        ],
        observations: vec![DecodedObservation; 2],
    });

    assert_eq!(verify(&fixture), Err(BundleFailure::Corrupt));
    assert_eq!(
        *fixture.trace.borrow(),
        vec!["presence", "reader", "header", "records"]
    );
}

#[test]
fn digest_algorithm_must_be_blake3() {
    let mut fixture = Fixture::new(vec![1], 1, 1);
    fixture.header_mut().digest_algorithm = "sha256".to_owned();

    assert_eq!(verify(&fixture), Err(BundleFailure::Corrupt));
    assert_eq!(
        *fixture.trace.borrow(),
        vec!["presence", "reader", "header"]
    );
}

#[test]
fn parser_failure_does_not_publish_a_partial_view() {
    let mut fixture = Fixture::new(vec![1], 1, 1);
    *fixture.decoded_mut() = Err(FormatReaderError);

    assert_eq!(verify(&fixture), Err(BundleFailure::Corrupt));
}
