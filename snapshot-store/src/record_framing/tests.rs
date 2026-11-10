use super::*;

fn record(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = ((payload.len() + 1) as u32).to_le_bytes().to_vec();
    bytes.push(kind);
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn accepts_each_chunk_boundary_and_maximum_length() {
    let mut data = record(1, b"volume");
    data.extend(record(2, &vec![7; 65_535]));
    data.extend(record(3, b"gap"));
    data.extend(record(0x7f, b""));
    for size in [1, 2, 3, 4, 5, 127, 16_384, 65_536] {
        let mut validator = Validator::new(4);
        for chunk in data.chunks(size) {
            assert!(validator.push(chunk).is_ok());
        }
        assert!(validator.finish().is_ok());
    }
}

#[test]
fn rejects_bad_lengths_order_unknown_and_end_trailing() {
    let vol = record(1, b"");
    let end = record(0x7f, b"");
    let cases = [
        vec![0, 0, 0, 0],
        65_537u32.to_le_bytes().to_vec(),
        record(2, b""),
        [vol.clone(), record(1, b"")].concat(),
        [vol.clone(), record(0xff, b"")].concat(),
        [vol.clone(), record(0x7f, b"x")].concat(),
        [vol.clone(), end.clone(), vec![0]].concat(),
        [vol.clone(), record(3, b""), record(2, b"")].concat(),
    ];
    for data in cases {
        assert!(Validator::new(10).push(&data).is_err(), "{data:?}");
    }
}

#[test]
fn rejects_truncated_headers_payloads_missing_end_and_bad_count() {
    let mut data = record(1, b"volume");
    data.extend(record(2, b"item"));
    data.extend(record(0x7f, b""));
    for end in 0..data.len() {
        let mut validator = Validator::new(3);
        assert!(
            validator.push(&data[..end]).is_err() || validator.finish().is_err(),
            "{end}"
        );
    }
    for count in [0, 1, 2, 4] {
        let mut validator = Validator::new(count);
        assert!(validator.push(&data).is_err() || validator.finish().is_err());
    }
}
