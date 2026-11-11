//! Read Windows local time-zone evidence at a supplied UTC instant.

use du_core_contracts::{
    Timestamp,
    time_zone::{LocalTimeZone, TimeZoneId, TimeZonePort, TimeZoneUnavailable},
};
use windows::Win32::{
    Foundation::{FILETIME, SYSTEMTIME},
    System::Time::{
        DYNAMIC_TIME_ZONE_INFORMATION, GetDynamicTimeZoneInformation, SystemTimeToFileTime,
        SystemTimeToTzSpecificLocalTimeEx, TIME_ZONE_ID_INVALID,
    },
};

/// Uses the active Windows time-zone settings without changing them.
pub struct WindowsTimeZone;

impl TimeZonePort for WindowsTimeZone {
    fn observe(&self, instant: &Timestamp) -> Result<LocalTimeZone, TimeZoneUnavailable> {
        let mut zone = DYNAMIC_TIME_ZONE_INFORMATION::default();
        // SAFETY: zone is a valid writable Windows structure for the duration of the call.
        if unsafe { GetDynamicTimeZoneInformation(&mut zone) } == TIME_ZONE_ID_INVALID {
            return Err(TimeZoneUnavailable);
        }
        observe_zone(&zone, instant)
    }
}

fn observe_zone(
    zone: &DYNAMIC_TIME_ZONE_INFORMATION,
    instant: &Timestamp,
) -> Result<LocalTimeZone, TimeZoneUnavailable> {
    let utc = system_time(instant)?;
    let mut local = SYSTEMTIME::default();
    // SAFETY: the supplied structures are initialized, live, and non-overlapping.
    unsafe { SystemTimeToTzSpecificLocalTimeEx(Some(zone), &utc, &mut local) }
        .map_err(|_| TimeZoneUnavailable)?;
    let utc_ticks = file_ticks(&utc)?;
    let local_ticks = file_ticks(&local)?;
    let seconds = (i128::from(local_ticks) - i128::from(utc_ticks)) / 10_000_000;
    let offset_seconds = i32::try_from(seconds).map_err(|_| TimeZoneUnavailable)?;
    if !(-86_400..=86_400).contains(&offset_seconds) {
        return Err(TimeZoneUnavailable);
    }
    let identity = decode_key(&zone.TimeZoneKeyName)?;
    Ok(LocalTimeZone {
        identity,
        offset_seconds,
    })
}

fn decode_key(key: &[u16]) -> Result<Option<TimeZoneId>, TimeZoneUnavailable> {
    let end = key
        .iter()
        .position(|unit| *unit == 0)
        .ok_or(TimeZoneUnavailable)?;
    if end == 0 {
        return Ok(None);
    }
    let key = String::from_utf16(&key[..end]).map_err(|_| TimeZoneUnavailable)?;
    Ok(Some(TimeZoneId::Windows(key)))
}

fn file_ticks(time: &SYSTEMTIME) -> Result<u64, TimeZoneUnavailable> {
    let mut file = FILETIME::default();
    // SAFETY: time is initialized and file is a valid writable output buffer.
    unsafe { SystemTimeToFileTime(time, &mut file) }.map_err(|_| TimeZoneUnavailable)?;
    Ok((u64::from(file.dwHighDateTime) << 32) | u64::from(file.dwLowDateTime))
}

fn system_time(instant: &Timestamp) -> Result<SYSTEMTIME, TimeZoneUnavailable> {
    let value = instant.as_str();
    let number = |start, end| {
        value
            .get(start..end)
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or(TimeZoneUnavailable)
    };
    Ok(SYSTEMTIME {
        wYear: number(0, 4)?,
        wMonth: number(5, 7)?,
        wDay: number(8, 10)?,
        wHour: number(11, 13)?,
        wMinute: number(14, 16)?,
        wSecond: number(17, 19)?,
        wMilliseconds: number(20, 23)?,
        wDayOfWeek: 0,
    })
}

#[cfg(test)]
mod tests;
