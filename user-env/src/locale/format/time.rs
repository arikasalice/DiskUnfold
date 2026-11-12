use super::*;

pub(super) fn format_time(request: &FormatRequest<'_>) -> FormatOutput {
    let RawValue::Time(state) = request.raw else {
        return unknown_kind_output();
    };
    let timestamp = match state {
        ValueState::Unknown { reason_key } => return unknown_output(reason_key.clone()),
        ValueState::Exact(value) | ValueState::LowerBound(value) => value,
    };
    let mut warnings = Vec::new();
    let (pattern, iso) = date_pattern(
        request.profile.date_format_id,
        &request.table,
        &mut warnings,
    );
    let (offset, timezone_unavailable) = time_zone_offset(
        request.profile.time_zone_policy,
        request.local_offset_seconds,
        &mut warnings,
    );
    let Some(civil) = timestamp_to_civil(timestamp, offset) else {
        let mut output = unknown_output("locale.value.invalid_time".to_owned());
        output.warnings = warnings;
        return output;
    };
    let date = format_date_pattern(pattern, civil).unwrap_or_else(|| {
        warnings.push("locale.format_unknown");
        format_date_pattern("yyyy-MM-dd HH:mm:ss", civil)
            .unwrap_or_else(|| "0000-00-00 00:00:00".to_owned())
    });
    let suffix = if iso {
        if offset == 0 {
            "Z".to_owned()
        } else {
            format_offset(offset)
        }
    } else if offset == 0 {
        " UTC".to_owned()
    } else {
        format!(" UTC{}", format_offset(offset))
    };
    let mut output = known_output(
        format!("{date}{suffix}"),
        ScreenReaderOutput::Time {
            date,
            offset_seconds: offset,
        },
        matches!(state, ValueState::LowerBound(_)),
    );
    if timezone_unavailable {
        output.markers.push(FormatMarker {
            key: "locale.timezone_unavailable",
            parameters: BTreeMap::new(),
        });
    }
    output.warnings = warnings;
    output
}

fn time_zone_offset(
    policy: &str,
    observed: Option<i32>,
    warnings: &mut Vec<&'static str>,
) -> (i32, bool) {
    match policy {
        "utc" => (0, false),
        "local" => match observed
            .filter(|offset| offset % 60 == 0 && offset.unsigned_abs() <= 23 * 60 * 60 + 59 * 60)
        {
            Some(offset) => (offset, false),
            None => (0, true),
        },
        _ => {
            warnings.push("locale.format_unknown");
            (0, false)
        }
    }
}

fn date_pattern<'a>(
    format_id: &str,
    table: &'a FormatTableView<'_>,
    warnings: &mut Vec<&'static str>,
) -> (&'a str, bool) {
    let selected_id = match format_id {
        "lang_default" | "iso8601" => format_id,
        _ => {
            warnings.push("locale.format_unknown");
            "lang_default"
        }
    };
    let pattern = table.date_patterns.get(selected_id).map(String::as_str);
    if let Some(pattern) = pattern.filter(|pattern| valid_pattern(pattern, table)) {
        return (pattern, selected_id == "iso8601");
    }
    warnings.push("locale.format_unknown");
    let fallback = table
        .date_patterns
        .get("lang_default")
        .map(String::as_str)
        .filter(|pattern| valid_pattern(pattern, table))
        .unwrap_or("yyyy-MM-dd HH:mm:ss");
    (fallback, false)
}

fn valid_pattern(pattern: &str, table: &FormatTableView<'_>) -> bool {
    let mut tokens = BTreeMap::<String, ()>::new();
    let mut allowed = BTreeMap::from([('-', ()), ('/', ()), ('.', ()), (':', ()), (' ', ())]);
    allowed.extend(
        table
            .decimal_separator
            .chars()
            .map(|character| (character, ())),
    );
    allowed.extend(
        table
            .grouping_separator
            .chars()
            .map(|character| (character, ())),
    );
    let mut chars = pattern.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\'' {
            if chars.next() != Some('T') || chars.next() != Some('\'') {
                return false;
            }
        } else if character.is_ascii_alphabetic() {
            let mut token = String::from(character);
            while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                if let Some(next) = chars.next() {
                    token.push(next);
                }
            }
            if !matches!(token.as_str(), "yyyy" | "MM" | "dd" | "HH" | "mm" | "ss")
                || tokens.insert(token, ()).is_some()
            {
                return false;
            }
        } else if !allowed.contains_key(&character) {
            return false;
        }
    }
    tokens.len() == 6
}

#[derive(Clone, Copy)]
struct CivilTime {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

fn timestamp_to_civil(timestamp: &Timestamp, offset_seconds: i32) -> Option<CivilTime> {
    let value = timestamp.as_str();
    let year = i64::from(parse_component(value, 0, 4)?);
    let month = parse_component(value, 5, 7)?;
    let day = parse_component(value, 8, 10)?;
    let hour = parse_component(value, 11, 13)?;
    let minute = parse_component(value, 14, 16)?;
    let second = parse_component(value, 17, 19)?;
    let days = days_from_civil(year, month, day)?;
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second))?
        .checked_add(i64::from(offset_seconds))?;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Some(CivilTime {
        year,
        month,
        day,
        hour: (day_seconds / 3600) as u32,
        minute: ((day_seconds % 3600) / 60) as u32,
        second: (day_seconds % 60) as u32,
    })
}

fn parse_component(value: &str, start: usize, end: usize) -> Option<u32> {
    value.get(start..end)?.parse().ok()
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 }.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    }
    .div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

fn format_date_pattern(pattern: &str, time: CivilTime) -> Option<String> {
    let mut output = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\'' {
            if chars.next() != Some('T') || chars.next() != Some('\'') {
                return None;
            }
            output.push('T');
        } else if character.is_ascii_alphabetic() {
            let mut token = String::from(character);
            while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                if let Some(next) = chars.next() {
                    token.push(next);
                }
            }
            match token.as_str() {
                "yyyy" => output.push_str(&format!("{:04}", time.year)),
                "MM" => output.push_str(&format!("{:02}", time.month)),
                "dd" => output.push_str(&format!("{:02}", time.day)),
                "HH" => output.push_str(&format!("{:02}", time.hour)),
                "mm" => output.push_str(&format!("{:02}", time.minute)),
                "ss" => output.push_str(&format!("{:02}", time.second)),
                _ => return None,
            }
        } else {
            output.push(character);
        }
    }
    Some(output)
}

fn format_offset(offset_seconds: i32) -> String {
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let minutes = offset_seconds.unsigned_abs() / 60;
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}
