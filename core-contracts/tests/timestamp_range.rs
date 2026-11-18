//! Checks the numeric timestamp range, its printed form and the strict text parser.
use du_core_contracts::Timestamp;

const MIN_MILLIS: i64 = -62_167_219_200_000;
const MAX_MILLIS: i64 = 253_402_300_799_999;

#[test]
fn known_instants_print_and_parse_canonically() -> Result<(), String> {
    let known = [
        (MIN_MILLIS, "0000-01-01T00:00:00.000Z"),
        (-1, "1969-12-31T23:59:59.999Z"),
        (0, "1970-01-01T00:00:00.000Z"),
        (951_782_400_000, "2000-02-29T00:00:00.000Z"),
        (1_000_000_000_123, "2001-09-09T01:46:40.123Z"),
        (1_709_164_800_000, "2024-02-29T00:00:00.000Z"),
        (4_107_542_400_000, "2100-02-28T00:00:00.000Z"),
        (MAX_MILLIS, "9999-12-31T23:59:59.999Z"),
    ];
    for (millis, text) in known {
        let built =
            Timestamp::from_unix_millis(millis).ok_or_else(|| format!("rejects {millis}"))?;
        assert_eq!(built.to_string(), text, "{millis}");
        assert_eq!(built.unix_millis(), millis);
        assert_eq!(Timestamp::parse(text)?, built, "{text}");
    }
    Ok(())
}

#[test]
fn from_unix_millis_rejects_instants_outside_four_digit_years() {
    assert!(Timestamp::from_unix_millis(MIN_MILLIS - 1).is_none());
    assert!(Timestamp::from_unix_millis(MAX_MILLIS + 1).is_none());
    assert!(Timestamp::from_unix_millis(i64::MIN).is_none());
    assert!(Timestamp::from_unix_millis(i64::MAX).is_none());
}

#[test]
fn parse_rejects_non_canonical_or_impossible_text() {
    let rejected = [
        "",
        "1970-01-01T00:00:00Z",
        "1970-01-01t00:00:00.000Z",
        "1970-01-01 00:00:00.000Z",
        "1970-01-01T00:00:00.000z",
        "1970-01-01T00:00:00,000Z",
        "1970-01-01T00:00:00.000+00:00",
        "1970-01-01T00:00:60.000Z",
        "1970-01-01T00:60:00.000Z",
        "1970-01-01T24:00:00.000Z",
        "1970-00-01T00:00:00.000Z",
        "1970-13-01T00:00:00.000Z",
        "1970-01-00T00:00:00.000Z",
        "1970-01-32T00:00:00.000Z",
        "1970-02-29T00:00:00.000Z",
        "1900-02-29T00:00:00.000Z",
        "2024-04-31T00:00:00.000Z",
        "+970-01-01T00:00:00.000Z",
        "1970-01-01T00:00:+0.000Z",
        "19700-1-01T00:00:00.000Z",
        "１970-01-01T00:00:00.000Z",
    ];
    for text in rejected {
        assert!(Timestamp::parse(text).is_err(), "{text}");
    }
}

#[test]
fn parse_round_trips_sampled_instants_across_the_range() -> Result<(), String> {
    let edges = [
        MIN_MILLIS,
        MAX_MILLIS,
        0,
        951_782_400_000,
        4_107_542_400_000,
    ];
    let mut samples: Vec<i64> = edges
        .iter()
        .flat_map(|edge| (-2_i64..=2).map(move |delta| edge + delta))
        .collect();
    let mut day = MIN_MILLIS;
    while day < MAX_MILLIS {
        samples.extend([day - 1, day, day + 86_399_999]);
        day += 86_400_000 * 97;
    }
    for millis in samples {
        let Some(time) = Timestamp::from_unix_millis(millis) else {
            continue;
        };
        assert_eq!(Timestamp::parse(&time.to_string())?, time, "{millis}");
    }
    Ok(())
}
