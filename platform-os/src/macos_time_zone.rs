//! Observe macOS local time-zone rules for a supplied UTC instant.

use du_core_contracts::{
    Timestamp,
    time_zone::{LocalTimeZone, TimeZoneId, TimeZonePort, TimeZoneUnavailable},
};
use objc2_foundation::{NSISO8601DateFormatOptions, NSISO8601DateFormatter, NSString, NSTimeZone};

/// Reads Foundation's local time-zone rules without changing system settings.
pub struct MacOsTimeZone;

impl TimeZonePort for MacOsTimeZone {
    fn observe(&self, instant: &Timestamp) -> Result<LocalTimeZone, TimeZoneUnavailable> {
        let zone = NSTimeZone::localTimeZone();
        observe_in(&zone, instant)
    }
}

fn observe_in(
    zone: &NSTimeZone,
    instant: &Timestamp,
) -> Result<LocalTimeZone, TimeZoneUnavailable> {
    let formatter = NSISO8601DateFormatter::new();
    formatter.setFormatOptions(
        NSISO8601DateFormatOptions::WithInternetDateTime
            | NSISO8601DateFormatOptions::WithFractionalSeconds,
    );
    let date = formatter
        .dateFromString(&NSString::from_str(instant.as_str()))
        .ok_or(TimeZoneUnavailable)?;
    let offset_seconds =
        i32::try_from(zone.secondsFromGMTForDate(&date)).map_err(|_| TimeZoneUnavailable)?;
    if !(-86_400..=86_400).contains(&offset_seconds) || offset_seconds % 60 != 0 {
        return Err(TimeZoneUnavailable);
    }
    Ok(LocalTimeZone {
        // The name comes from the same object that supplied the offset.
        identity: identity_from_name(&zone.name().to_string()),
        offset_seconds,
    })
}

/// Returns the Foundation identifier only when it names a zone rather than a fixed offset.
///
/// An empty name, a control character, or a fixed-offset form such as `GMT+0900` yields `None`
/// so that the observation stays offset-only instead of claiming an unverified identity.
fn identity_from_name(name: &str) -> Option<TimeZoneId> {
    if name.is_empty() || name.chars().any(char::is_control) || is_fixed_offset_name(name) {
        return None;
    }
    Some(TimeZoneId::MacOs(name.to_owned()))
}

fn is_fixed_offset_name(name: &str) -> bool {
    if name == "Z" || name.starts_with(['+', '-']) {
        return true;
    }
    let Some(rest) = name.strip_prefix("GMT") else {
        return false;
    };
    let mut chars = rest.chars();
    match chars.next() {
        None => true,
        Some('+' | '-') => chars.all(|c| c.is_ascii_digit() || c == ':'),
        Some(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observes_daylight_saving_rules_at_the_supplied_instant() -> Result<(), String> {
        let zone = NSTimeZone::timeZoneWithName(&NSString::from_str("America/New_York"))
            .ok_or("Foundation did not resolve the fixture time zone")?;
        let winter =
            Timestamp::parse("2026-01-15T12:00:00.000Z").map_err(|error| error.to_string())?;
        let summer =
            Timestamp::parse("2026-07-15T12:00:00.000Z").map_err(|error| error.to_string())?;
        let winter_offset = observe_in(&zone, &winter)
            .map_err(|_| "Foundation could not observe the winter fixture instant")?
            .offset_seconds;
        let summer_observation = observe_in(&zone, &summer)
            .map_err(|_| "Foundation could not observe the summer fixture instant")?;
        assert_eq!(winter_offset, -18_000);
        assert_eq!(summer_observation.offset_seconds, -14_400);
        assert_eq!(
            summer_observation.identity,
            Some(TimeZoneId::MacOs("America/New_York".into()))
        );
        Ok(())
    }

    #[test]
    fn fixed_offset_zones_stay_offset_only() -> Result<(), String> {
        let instant =
            Timestamp::parse("2026-01-15T12:00:00.000Z").map_err(|error| error.to_string())?;
        let fixed = NSTimeZone::timeZoneForSecondsFromGMT(32_400);
        let observed = observe_in(&fixed, &instant)
            .map_err(|_| "Foundation could not observe the fixed-offset fixture")?;
        assert_eq!(observed.offset_seconds, 32_400);
        assert_eq!(observed.identity, None);
        Ok(())
    }

    #[test]
    fn classifies_foundation_names() {
        for name in ["", "GMT", "GMT+0900", "GMT-05:30", "+09:00", "Z", "A\u{7}B"] {
            assert_eq!(identity_from_name(name), None, "{name:?}");
        }
        for name in ["America/New_York", "UTC", "GMT0", "Etc/GMT+9", "Asia/Seoul"] {
            assert_eq!(
                identity_from_name(name),
                Some(TimeZoneId::MacOs(name.into())),
                "{name:?}"
            );
        }
    }
}
