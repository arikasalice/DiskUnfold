//! Observe macOS local time-zone rules for a supplied UTC instant.

use du_core_contracts::{
    Timestamp,
    time_zone::{LocalTimeZone, TimeZonePort, TimeZoneUnavailable},
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
        // The shared contract has no macOS zone identifier variant yet.
        identity: None,
        offset_seconds,
    })
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
        assert_eq!(summer_observation.identity, None);
        Ok(())
    }
}
