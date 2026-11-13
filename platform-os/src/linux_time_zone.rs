//! Observe the Linux time zone using the same system library as the desktop shell.

use du_core_contracts::{
    Timestamp,
    time_zone::{LocalTimeZone, TimeZoneId, TimeZonePort, TimeZoneUnavailable},
};
use gtk4::glib::{DateTime, TimeZone};

/// Reads Linux local time without changing the process or system settings.
pub struct LinuxTimeZone;

impl TimeZonePort for LinuxTimeZone {
    fn observe(&self, instant: &Timestamp) -> Result<LocalTimeZone, TimeZoneUnavailable> {
        let configured = match std::env::var("TZ") {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => "/etc/localtime".to_owned(),
            Err(std::env::VarError::NotUnicode(_)) => return Err(TimeZoneUnavailable),
        };
        observe_configured(&configured, instant)
    }
}

fn observe_configured(
    configured: &str,
    instant: &Timestamp,
) -> Result<LocalTimeZone, TimeZoneUnavailable> {
    if configured.is_empty() {
        return Err(TimeZoneUnavailable);
    }
    // The GLib version exposed by the current GTK binding substitutes UTC for an
    // unrecognized zone. Accept an observation only when it returns the requested identifier.
    #[allow(deprecated)]
    let zone = TimeZone::new(Some(configured));
    let identifier = zone.identifier();
    if identifier.as_str() != configured {
        return Err(TimeZoneUnavailable);
    }
    let local = DateTime::from_iso8601(instant.as_str(), None)
        .and_then(|value| value.to_timezone(&zone))
        .map_err(|_| TimeZoneUnavailable)?;
    let offset_seconds =
        i32::try_from(local.utc_offset().as_seconds()).map_err(|_| TimeZoneUnavailable)?;
    if offset_seconds % 60 != 0 || offset_seconds.unsigned_abs() >= 86_400 {
        return Err(TimeZoneUnavailable);
    }
    let identity = (!configured.starts_with(['+', '-']) && configured != "Z")
        .then(|| TimeZoneId::Linux(identifier.to_string()));
    Ok(LocalTimeZone {
        identity,
        offset_seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observes_rules_for_the_supplied_instant_and_rejects_unknown_zones()
    -> Result<(), Box<dyn std::error::Error>> {
        let winter = Timestamp::parse("2026-01-15T12:00:00.000Z")?;
        let summer = Timestamp::parse("2026-07-15T12:00:00.000Z")?;
        let zone = "EST5EDT,M3.2.0,M11.1.0";
        let observe = |instant| observe_configured(zone, instant).map_err(|_| "zone unavailable");
        assert_eq!(observe(&winter)?.offset_seconds, -18_000);
        assert_eq!(observe(&summer)?.offset_seconds, -14_400);
        assert_eq!(
            observe(&winter)?.identity,
            Some(TimeZoneId::Linux(zone.to_owned()))
        );
        assert_eq!(
            observe_configured("DiskUnfold-invalid-timezone", &winter),
            Err(TimeZoneUnavailable),
        );
        Ok(())
    }
}
