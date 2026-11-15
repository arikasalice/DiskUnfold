//! Checks that the numeric timestamp range matches the text-based rule it replaced.
use du_core_contracts::Timestamp;

/// The rule used before the numeric representation: the formatted text must
/// pass the 24-character wire check and an RFC 3339 parse.
fn legacy_accepts(millis: i64) -> bool {
    let Some(time) = chrono::DateTime::from_timestamp_millis(millis) else {
        return false;
    };
    let text = time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    text.len() == 24
        && text.ends_with('Z')
        && text.as_bytes().get(19) == Some(&b'.')
        && chrono::DateTime::parse_from_rfc3339(&text).is_ok()
}

#[test]
fn from_unix_millis_matches_the_legacy_rule_and_prints_like_chrono() -> Result<(), String> {
    let edges = [
        -62_167_219_200_000_i64,
        253_402_300_799_999,
        0,
        951_782_400_000,
        4_107_542_400_000,
    ];
    let mut samples: Vec<i64> = edges
        .iter()
        .flat_map(|edge| (-2_i64..=2).map(move |delta| edge + delta))
        .collect();
    let mut millis = -62_300_000_000_000_i64;
    while millis < 253_500_000_000_000 {
        samples.push(millis);
        millis += 86_399_999_999;
    }
    let mut day = -62_167_219_200_000_i64;
    while day < 253_402_300_800_000 {
        samples.extend([day - 1, day, day + 86_399_999]);
        day += 86_400_000 * 97;
    }
    for millis in samples {
        let built = Timestamp::from_unix_millis(millis);
        assert_eq!(built.is_some(), legacy_accepts(millis), "{millis}");
        if let Some(time) = built {
            let expected = chrono::DateTime::from_timestamp_millis(millis)
                .ok_or_else(|| format!("chrono rejects {millis}"))?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            assert_eq!(time.to_string(), expected, "{millis}");
            assert_eq!(time.unix_millis(), millis);
            assert_eq!(Timestamp::parse(&expected)?, time);
        }
    }
    Ok(())
}
