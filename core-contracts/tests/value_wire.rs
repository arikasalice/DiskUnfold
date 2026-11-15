//! Pins the JSON wire form of `ByteSize` and `Timestamp` so the internal
//! representation can change without altering what clients see.
#![allow(clippy::expect_used)]

use du_core_contracts::{ByteSize, SizeAmount, SizeBasis, SizeEvidence, SizeValue, Timestamp};
use std::hash::{DefaultHasher, Hash, Hasher};

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn byte_json(text: &str) -> String {
    serde_json::to_string(text).expect("string json")
}

const MIN_MILLIS: i64 = -62_167_219_200_000;
const MAX_MILLIS: i64 = 253_402_300_799_999;

#[test]
fn byte_size_accepts_canonical_decimal_text() {
    for text in ["0", "1", "123", "18446744073709551615"] {
        let value = ByteSize::parse(text).expect(text);
        let json = serde_json::to_string(&value).expect("serialize");
        assert_eq!(json, byte_json(text));
        let back: ByteSize = serde_json::from_str(&json).expect("round trip");
        assert_eq!(back, value);
        assert_eq!(serde_json::to_string(&back).expect("serialize"), json);
        let decoded: ByteSize = serde_json::from_str(&byte_json(text)).expect("deserialize");
        assert_eq!(decoded, value);
        assert_eq!(value.to_string(), text);
    }
}

#[test]
fn byte_size_rejects_non_canonical_text() {
    for text in [
        "",
        "01",
        "00",
        "+1",
        "-1",
        " 1",
        "1 ",
        "1.0",
        "0x10",
        "18446744073709551616",
    ] {
        assert!(ByteSize::parse(text).is_err(), "parse accepted {text:?}");
        assert!(
            serde_json::from_str::<ByteSize>(&byte_json(text)).is_err(),
            "deserialize accepted {text:?}"
        );
    }
}

#[test]
fn byte_size_rejects_non_string_json() {
    assert!(serde_json::from_str::<ByteSize>("1").is_err());
    assert!(serde_json::from_str::<ByteSize>("null").is_err());
    assert!(serde_json::from_str::<ByteSize>("true").is_err());
}

#[test]
fn byte_size_from_u64_serializes_as_decimal_string() {
    for (number, text) in [
        (0_u64, "0"),
        (7, "7"),
        (1_000_000, "1000000"),
        (u64::MAX, "18446744073709551615"),
    ] {
        let value = ByteSize::from(number);
        assert_eq!(
            serde_json::to_string(&value).expect("serialize"),
            byte_json(text)
        );
        assert_eq!(value, ByteSize::parse(text).expect("parse"));
    }
}

#[test]
fn byte_size_equality_and_hash_follow_the_value() {
    let left = ByteSize::parse("4096").expect("parse");
    let right = ByteSize::from(4096_u64);
    assert_eq!(left, right);
    assert_eq!(hash_of(&left), hash_of(&right));
    assert_ne!(left, ByteSize::from(4097_u64));
}

#[test]
fn timestamp_accepts_canonical_form() {
    for text in [
        "2026-11-15T00:00:00.000Z",
        "1970-01-01T00:00:00.000Z",
        "2024-02-29T12:34:56.789Z",
    ] {
        let value = Timestamp::parse(text).expect(text);
        let json = serde_json::to_string(&value).expect("serialize");
        assert_eq!(json, byte_json(text));
        let back: Timestamp = serde_json::from_str(&json).expect("round trip");
        assert_eq!(back, value);
        assert_eq!(serde_json::to_string(&back).expect("serialize"), json);
        assert_eq!(hash_of(&back), hash_of(&value));
    }
}

#[test]
fn timestamp_accepts_the_year_range_ends() {
    for text in ["0000-01-01T00:00:00.000Z", "9999-12-31T23:59:59.999Z"] {
        let value = Timestamp::parse(text).expect(text);
        assert_eq!(
            serde_json::to_string(&value).expect("serialize"),
            byte_json(text)
        );
    }
}

#[test]
fn timestamp_rejects_malformed_text() {
    for text in [
        "",
        "2026-11-15T00:00:00Z",
        "2026-11-15T00:00:00.0000Z",
        "2026-11-15T00:00:00.00Z",
        "2026-11-15T00:00:00.000+00:00",
        "2026-11-15T00:00:00.000z",
        "2026-11-15T00:00:00,000Z",
        "2026-11-15T00:00:00:000Z",
        "2026-13-15T00:00:00.000Z",
        "2026-00-15T00:00:00.000Z",
        "2026-11-32T00:00:00.000Z",
        "2026-11-00T00:00:00.000Z",
        "2026-02-30T00:00:00.000Z",
        "2026-11-15T24:00:00.000Z",
        "2026-11-15T00:60:00.000Z",
        "2026-11-15T00:00:61.000Z",
        " 026-11-15T00:00:00.000Z",
    ] {
        assert!(Timestamp::parse(text).is_err(), "parse accepted {text:?}");
        assert!(
            serde_json::from_str::<Timestamp>(&byte_json(text)).is_err(),
            "deserialize accepted {text:?}"
        );
    }
    assert!(serde_json::from_str::<Timestamp>("0").is_err());
    assert!(serde_json::from_str::<Timestamp>("null").is_err());
}

#[test]
fn timestamp_from_unix_millis_covers_exactly_the_four_digit_years() {
    assert_eq!(
        serde_json::to_string(&Timestamp::from_unix_millis(0).expect("epoch")).expect("json"),
        byte_json("1970-01-01T00:00:00.000Z")
    );
    let first = Timestamp::from_unix_millis(MIN_MILLIS).expect("first millisecond");
    assert_eq!(
        serde_json::to_string(&first).expect("json"),
        byte_json("0000-01-01T00:00:00.000Z")
    );
    let last = Timestamp::from_unix_millis(MAX_MILLIS).expect("last millisecond");
    assert_eq!(
        serde_json::to_string(&last).expect("json"),
        byte_json("9999-12-31T23:59:59.999Z")
    );
    assert!(Timestamp::from_unix_millis(MIN_MILLIS - 1).is_none());
    assert!(Timestamp::from_unix_millis(MAX_MILLIS + 1).is_none());
    assert!(Timestamp::from_unix_millis(i64::MIN).is_none());
    assert!(Timestamp::from_unix_millis(i64::MAX).is_none());
}

#[test]
fn timestamp_from_unix_millis_matches_parse_for_sampled_values() {
    for millis in [-1_i64, 1, 999, 1_000, 1_763_164_800_123, MAX_MILLIS - 1] {
        let built = Timestamp::from_unix_millis(millis).expect("in range");
        let json = serde_json::to_string(&built).expect("json");
        let parsed: Timestamp = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed, built);
    }
}

// The three inputs below are not the canonical form this crate produces. They
// record what the implementation did when the wire form was pinned.
#[test]
fn timestamp_leap_second_text_current_behaviour() {
    let text = "2016-12-31T23:59:60.000Z";
    let value = Timestamp::parse(text).expect("accepted at this commit");
    assert_eq!(
        serde_json::to_string(&value).expect("json"),
        byte_json(text)
    );
    assert!(serde_json::from_str::<Timestamp>(&byte_json(text)).is_ok());
}

#[test]
fn timestamp_lowercase_t_separator_current_behaviour() {
    let text = "2026-11-15t00:00:00.000Z";
    let value = Timestamp::parse(text).expect("accepted at this commit");
    assert_eq!(
        serde_json::to_string(&value).expect("json"),
        byte_json(text)
    );
    assert!(serde_json::from_str::<Timestamp>(&byte_json(text)).is_ok());
}

#[test]
fn timestamp_space_separator_current_behaviour() {
    let text = "2026-11-15 00:00:00.000Z";
    let value = Timestamp::parse(text).expect("accepted at this commit");
    assert_eq!(
        serde_json::to_string(&value).expect("json"),
        byte_json(text)
    );
    assert!(serde_json::from_str::<Timestamp>(&byte_json(text)).is_ok());
}

#[test]
fn size_value_wire_shape_is_pinned() {
    let exact = SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount: SizeAmount::Exact {
            bytes: ByteSize::from(123_u64),
        },
    };
    assert_eq!(
        serde_json::to_string(&exact).expect("json"),
        r#"{"basis":"logical","evidence":"measured","state":"exact","bytes":"123"}"#
    );
    let back: SizeValue = serde_json::from_str(
        r#"{"basis":"logical","evidence":"measured","state":"exact","bytes":"123"}"#,
    )
    .expect("deserialize");
    assert_eq!(back, exact);
}

#[test]
fn size_value_unknown_has_no_bytes() {
    let unknown = SizeValue {
        basis: SizeBasis::Allocated,
        evidence: SizeEvidence::Unknown,
        amount: SizeAmount::Unknown,
    };
    assert_eq!(
        serde_json::to_string(&unknown).expect("json"),
        r#"{"basis":"allocated","evidence":"unknown","state":"unknown"}"#
    );
    let with_bytes = r#"{"basis":"allocated","evidence":"unknown","state":"unknown","bytes":"1"}"#;
    assert!(serde_json::from_str::<SizeValue>(with_bytes).is_err());
}
