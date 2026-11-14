import { formatByteUnit } from "../../i18n/bytes";
import type { SizeDisplay } from "../../i18n/bytes";
import type { LocaleText } from "../../i18n/presentation";
import { SIZE_UNIT_RULES } from "../../ipc";
import type { BrowseValue, SizeBasis, SizeValue } from "../../ipc";

function byteText(bytes: string, display: SizeDisplay, lowerBound: boolean) {
  return formatByteUnit(
    BigInt(bytes),
    SIZE_UNIT_RULES[display.system],
    SIZE_UNIT_RULES.decimalPlaces,
    display.unit,
    lowerBound,
    display.regional,
  );
}

/**
 * Shows a size. A lower bound reads "or more" and an unknown size reads
 * "unknown"; neither is ever shown as zero.
 */
export function sizeValueText(
  value: SizeValue,
  display: SizeDisplay,
  text: LocaleText,
): string {
  if (value.state === "unknown")
    return text("ui.groups.size_unknown", "Unknown");
  if (value.state === "exact") return byteText(value.bytes, display, false);
  return text("ui.groups.size_at_least", "{size} or more", {
    size: byteText(value.bytes, display, true),
  });
}

/** Shows a member size, keeping "not recorded" apart from "unknown". */
export function memberSizeText(
  value: BrowseValue<SizeValue>,
  display: SizeDisplay,
  text: LocaleText,
): string {
  if (value.state === "absent")
    return text("ui.browse.not_recorded", "Not recorded");
  if (value.state === "unknown")
    return text("ui.groups.size_unknown", "Unknown");
  return sizeValueText(value.value, display, text);
}

/** Shows a text value of a member row; absent and unknown read differently. */
export function memberTextValue(
  value: BrowseValue<string>,
  text: LocaleText,
): string {
  if (value.state === "present") return value.value;
  return value.state === "absent"
    ? text("ui.browse.not_recorded", "Not recorded")
    : text("ui.browse.unavailable", "Unavailable");
}

/**
 * Converts an exact integer pair to a percentage at display time only. The
 * value is truncated, so a share below 100% is never shown as 100%, and a
 * nonzero share is never shown as 0%.
 */
export function percentText(
  numerator: string,
  denominator: string,
  text: LocaleText,
): string | undefined {
  if (!/^\d+$/.test(numerator) || !/^\d+$/.test(denominator)) return undefined;
  const top = BigInt(numerator);
  const bottom = BigInt(denominator);
  if (bottom === 0n) return undefined;
  const tenths = (top * 1000n) / bottom;
  if (top > 0n && tenths === 0n)
    return text("ui.groups.percent_under", "less than {value}%", {
      value: "0.1",
    });
  const value =
    tenths % 10n === 0n
      ? String(tenths / 10n)
      : `${tenths / 10n}.${tenths % 10n}`;
  return text("ui.groups.percent", "{value}%", { value });
}

/** The label of a size basis. */
export function basisLabel(basis: SizeBasis, text: LocaleText): string {
  if (basis === "logical")
    return text("ui.groups.basis_logical", "Logical size");
  if (basis === "allocated")
    return text("ui.groups.basis_allocated", "Allocated size");
  return text("ui.groups.basis_unique", "Unique physical size");
}
