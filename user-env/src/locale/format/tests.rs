use super::*;
use serde_json::{Value, json};

#[test]
fn shared_vectors_match_display_speech_markers_and_warnings() -> Result<(), String> {
    let fixture: Value = serde_json::from_str(include_str!("../../../i18n/format-vectors.json"))
        .map_err(|error| error.to_string())?;
    let cases = fixture["cases"].as_array().ok_or("missing cases")?;
    for case in cases {
        let case_id = text(&case["id"])?;
        let table = &fixture["tables"][text(&case["table"])?];
        let date_patterns = string_map(&table["datePatterns"])?;
        let unit_symbols = string_map(&table["unitSymbols"])?;
        let format_table = FormatTableView {
            decimal_separator: text(&table["decimalSeparator"])?,
            grouping_separator: text(&table["groupingSeparator"])?,
            grouping_size: usize::try_from(
                table["groupingSize"]
                    .as_u64()
                    .ok_or("invalid grouping size")?,
            )
            .map_err(|error| error.to_string())?,
            date_patterns: &date_patterns,
            unit_symbols: &unit_symbols,
        };
        let profile_value = &case["profile"];
        let profile = FormatProfile {
            number_format_id: text(&profile_value["numberFormatId"])?,
            date_format_id: text(&profile_value["dateFormatId"])?,
            time_zone_policy: text(&profile_value["timeZonePolicy"])?,
            size_unit_system: text(&profile_value["sizeUnitSystem"])?,
            size_unit: text(&profile_value["sizeUnit"])?,
        };
        let raw = parse_raw(&case["raw"])?;
        let offset = case["localOffsetSeconds"]
            .as_i64()
            .map(|value| i32::try_from(value).map_err(|error| error.to_string()))
            .transpose()?;
        let actual = output_json(&format_raw(
            text(&case["formatId"])?,
            &raw,
            &profile,
            &format_table,
            offset,
        ));
        assert_eq!(actual, case["expected"], "vector {case_id}");
    }
    Ok(())
}

fn text(value: &Value) -> Result<&str, String> {
    value.as_str().ok_or_else(|| "expected string".to_owned())
}

fn string_map(value: &Value) -> Result<BTreeMap<String, String>, String> {
    let fields = value.as_object().ok_or("expected string map")?;
    fields
        .iter()
        .map(|(key, value)| Ok((key.clone(), text(value)?.to_owned())))
        .collect()
}

fn parse_raw(value: &Value) -> Result<RawValue, String> {
    let kind = text(&value["kind"])?;
    let state = text(&value["state"])?;
    match kind {
        "count" => Ok(RawValue::Count(parse_state(state, value, |value| {
            text(value)?
                .parse::<u64>()
                .map_err(|error| error.to_string())
        })?)),
        "capacity" => Ok(RawValue::Capacity(parse_state(state, value, |value| {
            text(value)?
                .parse::<u64>()
                .map_err(|error| error.to_string())
        })?)),
        "number" => Ok(RawValue::Number(parse_state(state, value, |_| {
            let coefficient = text(&value["value"]["coefficient"])?
                .parse::<i128>()
                .map_err(|error| error.to_string())?;
            let scale = u8::try_from(
                value["value"]["scale"]
                    .as_u64()
                    .ok_or("invalid number scale")?,
            )
            .map_err(|error| error.to_string())?;
            Decimal::new(coefficient, scale).ok_or_else(|| "unsupported number scale".to_owned())
        })?)),
        "time" => Ok(RawValue::Time(parse_state(state, value, |value| {
            Timestamp::parse(text(value)?).map_err(|error| error.to_string())
        })?)),
        _ => Err("unknown raw kind".to_owned()),
    }
}

fn parse_state<T>(
    state: &str,
    value: &Value,
    parse: impl FnOnce(&Value) -> Result<T, String>,
) -> Result<ValueState<T>, String> {
    match state {
        "exact" => Ok(ValueState::Exact(parse(&value["value"])?)),
        "lower_bound" => Ok(ValueState::LowerBound(parse(&value["value"])?)),
        "unknown" => Ok(ValueState::Unknown {
            reason_key: text(&value["reasonKey"])?.to_owned(),
        }),
        _ => Err("unknown value state".to_owned()),
    }
}

fn output_json(output: &FormatOutput) -> Value {
    let visual = match &output.visual {
        FormatText::Literal(text) => json!({"kind":"literal", "text":text}),
        FormatText::Message { key, parameters } => {
            json!({"kind":"message", "key":key, "parameters":parameters})
        }
    };
    let screen_reader = match &output.screen_reader {
        ScreenReaderOutput::Plain(text) => json!({"kind":"plain", "text":text}),
        ScreenReaderOutput::Quantity { number, unit_id } => {
            json!({"kind":"quantity", "number":number, "unitId":unit_id})
        }
        ScreenReaderOutput::Time {
            date,
            offset_seconds,
        } => json!({"kind":"time", "date":date, "offsetSeconds":offset_seconds}),
        ScreenReaderOutput::Message { key, parameters } => {
            json!({"kind":"message", "key":key, "parameters":parameters})
        }
    };
    let markers = output
        .markers
        .iter()
        .map(|marker| json!({"key":marker.key, "parameters":marker.parameters}))
        .collect::<Vec<_>>();
    json!({
        "visual":visual,
        "screenReader":screen_reader,
        "markers":markers,
        "unknownReason":output.unknown_reason,
        "warnings":output.warnings
    })
}
