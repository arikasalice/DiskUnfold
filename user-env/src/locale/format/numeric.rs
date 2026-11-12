use super::*;

pub(super) fn format_count(request: &FormatRequest<'_>) -> FormatOutput {
    let RawValue::Count(state) = request.raw else {
        return unknown_kind_output();
    };
    match state {
        ValueState::Unknown { reason_key } => unknown_output(reason_key.clone()),
        ValueState::Exact(value) => count_output(request, *value, false),
        ValueState::LowerBound(value) => count_output(request, *value, true),
    }
}

fn count_output(request: &FormatRequest<'_>, value: u64, lower_bound: bool) -> FormatOutput {
    let mut warnings = Vec::new();
    let grouped = use_grouping(request.profile.number_format_id, &mut warnings);
    let separators = number_separators(&request.table, &mut warnings);
    let text = format_integer(value.to_string(), grouped, separators);
    let mut output = known_output(
        text,
        ScreenReaderOutput::Plain(value.to_string()),
        lower_bound,
    );
    output.warnings = warnings;
    output
}

pub(super) fn format_number_value(request: &FormatRequest<'_>) -> FormatOutput {
    let RawValue::Number(state) = request.raw else {
        return unknown_kind_output();
    };
    match state {
        ValueState::Unknown { reason_key } => unknown_output(reason_key.clone()),
        ValueState::Exact(value) => decimal_output(request, *value, false),
        ValueState::LowerBound(value) => decimal_output(request, *value, true),
    }
}

fn decimal_output(request: &FormatRequest<'_>, value: Decimal, lower_bound: bool) -> FormatOutput {
    let mut warnings = Vec::new();
    let grouped = use_grouping(request.profile.number_format_id, &mut warnings);
    let separators = number_separators(&request.table, &mut warnings);
    let visual = format_decimal(value, grouped, separators);
    let spoken = format_decimal(value, false, separators);
    let mut output = known_output(visual, ScreenReaderOutput::Plain(spoken), lower_bound);
    output.warnings = warnings;
    output
}

pub(super) fn format_capacity(request: &FormatRequest<'_>) -> FormatOutput {
    let RawValue::Capacity(state) = request.raw else {
        return unknown_kind_output();
    };
    let (bytes, lower_bound) = match state {
        ValueState::Unknown { reason_key } => return unknown_output(reason_key.clone()),
        ValueState::Exact(value) => (*value, false),
        ValueState::LowerBound(value) => (*value, true),
    };
    let mut warnings = Vec::new();
    let grouped = use_grouping(request.profile.number_format_id, &mut warnings);
    let separators = number_separators(&request.table, &mut warnings);
    let unit_table = size_unit_table(request.profile.size_unit_system, &mut warnings);
    let unit_index = selected_unit(bytes, request.profile.size_unit, unit_table, &mut warnings);
    let unit_id = unit_table.symbols()[unit_index];
    let symbol = request
        .table
        .unit_symbols
        .get(unit_id)
        .map(String::as_str)
        .unwrap_or_else(|| {
            warnings.push("locale.format_unknown");
            unit_id
        });
    let number = size_number(bytes, unit_table.base(), unit_index, lower_bound);
    let visual_number = format_tenths_or_integer(number, unit_index, grouped, separators);
    let spoken_number = format_tenths_or_integer(number, unit_index, false, separators);
    let visual = format!("{visual_number} {symbol}");
    let spoken = ScreenReaderOutput::Quantity {
        number: spoken_number,
        unit_id: unit_id.to_owned(),
    };
    let mut output = known_output(visual, spoken, lower_bound);
    output.warnings = warnings;
    output
}

fn size_unit_table(
    system_id: &str,
    warnings: &mut Vec<&'static str>,
) -> &'static du_core_contracts::size_units::UnitTable {
    match system_id {
        "iec" => &IEC_UNITS,
        "si" => &SI_UNITS,
        _ => {
            warnings.push("locale.format_unknown");
            &SI_UNITS
        }
    }
}

fn selected_unit(
    bytes: u64,
    selected: &str,
    table: &du_core_contracts::size_units::UnitTable,
    warnings: &mut Vec<&'static str>,
) -> usize {
    if selected == "auto" {
        return automatic_unit(bytes, table);
    }
    if let Some(index) = table
        .symbols()
        .iter()
        .position(|symbol| *symbol == selected)
    {
        return index;
    }
    warnings.push("locale.format_unknown");
    automatic_unit(bytes, table)
}

fn automatic_unit(bytes: u64, table: &du_core_contracts::size_units::UnitTable) -> usize {
    let value = u128::from(bytes);
    let base = u128::from(table.base());
    (1..table.symbols().len())
        .take_while(|index| {
            base.checked_pow(*index as u32)
                .is_some_and(|bound| value >= bound)
        })
        .last()
        .unwrap_or(0)
}

fn size_number(bytes: u64, base: u16, unit_index: usize, lower_bound: bool) -> u128 {
    let decimal_scale = 10_u128.pow(DISPLAY_DECIMAL_PLACES);
    if unit_index == 0 {
        return u128::from(bytes) * decimal_scale;
    }
    let divisor = u128::from(base).pow(unit_index as u32);
    let scaled = u128::from(bytes) * decimal_scale;
    let rounding = if lower_bound { 0 } else { divisor / 2 };
    (scaled + rounding) / divisor
}

fn format_tenths_or_integer(
    value_tenths: u128,
    unit_index: usize,
    grouped: bool,
    separators: NumberSeparators<'_>,
) -> String {
    let decimal_scale = 10_u128.pow(DISPLAY_DECIMAL_PLACES);
    if unit_index == 0 {
        return format_integer(
            (value_tenths / decimal_scale).to_string(),
            grouped,
            separators,
        );
    }
    let integer = (value_tenths / decimal_scale).to_string();
    let fractional = value_tenths % decimal_scale;
    let width = DISPLAY_DECIMAL_PLACES as usize;
    let fraction = (fractional != 0).then(|| format!("{fractional:0width$}"));
    format_decimal_parts(integer, fraction, false, grouped, separators)
}

#[derive(Clone, Copy)]
struct NumberSeparators<'a> {
    decimal: &'a str,
    grouping: &'a str,
    size: usize,
}

fn number_separators<'a>(
    table: &'a FormatTableView<'_>,
    warnings: &mut Vec<&'static str>,
) -> NumberSeparators<'a> {
    let (decimal, grouping) = if table.decimal_separator.is_empty()
        || table.grouping_separator.is_empty()
        || table.decimal_separator == table.grouping_separator
    {
        warnings.push("locale.format_unknown");
        (".", ",")
    } else {
        (table.decimal_separator, table.grouping_separator)
    };
    let size = if table.grouping_size == 0 {
        warnings.push("locale.format_unknown");
        3
    } else {
        table.grouping_size
    };
    NumberSeparators {
        decimal,
        grouping,
        size,
    }
}

fn use_grouping(format_id: &str, warnings: &mut Vec<&'static str>) -> bool {
    match format_id {
        "lang_default" => true,
        "no_grouping" => false,
        _ => {
            warnings.push("locale.format_unknown");
            true
        }
    }
}

fn format_integer(integer: String, grouped: bool, separators: NumberSeparators<'_>) -> String {
    if !grouped || integer.len() <= separators.size {
        return integer;
    }
    let first = integer.len() % separators.size;
    let mut output = String::with_capacity(integer.len() + integer.len() / separators.size);
    for (index, character) in integer.chars().enumerate() {
        if (index == first && first != 0)
            || (index > first && (index - first) % separators.size == 0)
        {
            output.push_str(separators.grouping);
        }
        output.push(character);
    }
    output
}

fn format_decimal(value: Decimal, grouped: bool, separators: NumberSeparators<'_>) -> String {
    let negative = value.coefficient < 0;
    let digits = value.coefficient.unsigned_abs().to_string();
    let scale = usize::from(value.scale);
    let (integer, fraction) = if scale == 0 {
        (digits, None)
    } else {
        let padded = if digits.len() <= scale {
            format!("{}{}", "0".repeat(scale + 1 - digits.len()), digits)
        } else {
            digits
        };
        let split = padded.len() - scale;
        let integer = padded[..split].to_owned();
        let fractional = &padded[split..];
        let mut tenth = fractional.as_bytes().first().copied().unwrap_or(b'0');
        let round_up = fractional
            .as_bytes()
            .get(1)
            .is_some_and(|digit| *digit >= b'5');
        let mut integer = integer;
        if round_up {
            if tenth == b'9' {
                tenth = b'0';
                integer = increment_decimal_digits(integer);
            } else {
                tenth += 1;
            }
        }
        (
            integer,
            (tenth != b'0').then(|| char::from(tenth).to_string()),
        )
    };
    format_decimal_parts(integer, fraction, negative, grouped, separators)
}

fn increment_decimal_digits(digits: String) -> String {
    let mut characters = digits.chars().collect::<Vec<_>>();
    let mut carry = true;
    for digit in characters.iter_mut().rev() {
        if *digit < '9' {
            *digit = char::from(*digit as u8 + 1);
            carry = false;
            break;
        }
        *digit = '0';
    }
    if carry {
        characters.insert(0, '1');
    }
    characters.into_iter().collect()
}

fn format_decimal_parts(
    integer: String,
    fraction: Option<String>,
    negative: bool,
    grouped: bool,
    separators: NumberSeparators<'_>,
) -> String {
    let fraction_is_zero = fraction
        .as_deref()
        .is_none_or(|digits| digits.bytes().all(|digit| digit == b'0'));
    let zero = integer.bytes().all(|digit| digit == b'0') && fraction_is_zero;
    let sign = if negative && !zero { "-" } else { "" };
    let mut output = format!("{sign}{}", format_integer(integer, grouped, separators));
    if let Some(fraction) = fraction {
        output.push_str(separators.decimal);
        output.push_str(&fraction);
    }
    output
}
