import type { SizeValue, SpaceTotal } from "../../ipc";
import { SIZE_UNIT_RULES } from "../../ipc";
import { formatByteUnit } from "../../i18n/bytes";

export type SizeDisplay = {
  system: "iec" | "si";
  unit: number | "auto";
};

export const windowsSizeDisplay: SizeDisplay = { system: "iec", unit: "auto" };

export type SizeBasisFilter = "all" | SizeValue["basis"];
export const sizeLabels = {
  logical: "Logical size",
  allocated: "Allocated size",
  unique_physical: "Unique physical size",
} satisfies Record<SizeValue["basis"], string>;

const measurementLabels = {
  complete: "Complete measurement",
  partial: "Partial measurement",
  missing: "Incomplete measurement",
  cancelled: "Measurement interrupted",
} satisfies Record<SpaceTotal["completeness"], string>;

/** Human-readable evidence labels shared by the two space views. */
export const evidenceLabels = {
  measured: "Measured",
  os_reported: "Reported by the file system",
  estimated: "Estimated",
  unknown: undefined,
} satisfies Record<SizeValue["evidence"], string | undefined>;

/** Separates measurement completeness and evidence from the result version. */
export function displayMeasurement(
  value: SizeValue,
  completeness: SpaceTotal["completeness"],
): string {
  return [measurementLabels[completeness], evidenceLabels[value.evidence]]
    .filter(Boolean)
    .join(" · ");
}

/** Formats exact wire bytes without converting them to a floating-point number. */
export function formatBytes(bytes: string): string {
  return `${BigInt(bytes).toLocaleString()} bytes`;
}

/** Converts a wire byte count using exact arithmetic before display rounding. */
export function formatSizeBytes(
  bytes: string,
  display: SizeDisplay,
  lowerBound = false,
): string {
  return formatByteUnit(
    BigInt(bytes),
    SIZE_UNIT_RULES[display.system],
    SIZE_UNIT_RULES.decimalPlaces,
    display.unit,
    lowerBound,
  );
}

/** Preserves a lower bound or missing observation in the displayed size. */
export function displaySize(
  size: SizeValue,
  display: SizeDisplay = windowsSizeDisplay,
): string {
  if (size.state === "unknown") return "Unavailable";
  const lowerBound = size.state === "lower_bound";
  const formatted = formatSizeBytes(size.bytes, display, lowerBound);
  return lowerBound ? `At least ${formatted}` : formatted;
}
