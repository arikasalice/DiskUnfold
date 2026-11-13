use super::*;

fn instant(value: &str) -> Result<Timestamp, String> {
    Timestamp::parse(value)
}

#[test]
fn supplied_instant_uses_dst_and_sign_of_offset() -> Result<(), String> {
    let mut zone = DYNAMIC_TIME_ZONE_INFORMATION::default();
    let mut found = false;
    for index in 0..2048 {
        // SAFETY: zone is writable and the index is an ordinary enumeration value.
        if unsafe { windows::Win32::System::Time::EnumDynamicTimeZoneInformation(index, &mut zone) }
            != 0
        {
            break;
        }
        if decode_key(&zone.TimeZoneKeyName)
            == Ok(Some(TimeZoneId::Windows("Eastern Standard Time".into())))
        {
            found = true;
            break;
        }
    }
    if !found {
        return Err("Windows Eastern Standard Time fixture unavailable".into());
    }
    let winter =
        observe_zone(&zone, &instant("2026-01-15T12:00:00.000Z")?).map_err(|e| format!("{e:?}"))?;
    let summer =
        observe_zone(&zone, &instant("2026-07-15T12:00:00.000Z")?).map_err(|e| format!("{e:?}"))?;
    assert_eq!(winter.offset_seconds, -5 * 3600);
    assert_eq!(summer.offset_seconds, -4 * 3600);
    assert_eq!(
        winter.identity,
        Some(TimeZoneId::Windows("Eastern Standard Time".into()))
    );
    Ok(())
}

#[test]
fn malformed_key_is_not_claimed_as_identified() {
    assert!(decode_key(&[65; 128]).is_err());
    assert!(decode_key(&[0xd800, 0]).is_err());
    assert_eq!(decode_key(&[0]), Ok(None));
    assert_eq!(
        decode_key(&[b'K' as u16, 0]),
        Ok(Some(TimeZoneId::Windows("K".into())))
    );
}

#[test]
fn utc_parsing_preserves_milliseconds_and_rejects_unrepresentable_year() -> Result<(), String> {
    let value =
        system_time(&instant("2026-12-31T23:59:58.123Z")?).map_err(|error| format!("{error:?}"))?;
    assert_eq!((value.wYear, value.wMonth, value.wDay), (2026, 12, 31));
    assert_eq!(
        (
            value.wHour,
            value.wMinute,
            value.wSecond,
            value.wMilliseconds
        ),
        (23, 59, 58, 123)
    );
    let early =
        system_time(&instant("0001-01-01T00:00:00.000Z")?).map_err(|error| format!("{error:?}"))?;
    assert!(file_ticks(&early).is_err());
    Ok(())
}

#[test]
fn real_windows_observation_returns_native_evidence() -> Result<(), String> {
    let observed = WindowsTimeZone
        .observe(&instant("2026-01-15T12:00:00.000Z")?)
        .map_err(|e| format!("native time zone unavailable: {e:?}"))?;
    assert!((-86_400..=86_400).contains(&observed.offset_seconds));
    if let Some(TimeZoneId::Windows(key)) = observed.identity {
        assert!(!key.is_empty());
    }
    Ok(())
}
