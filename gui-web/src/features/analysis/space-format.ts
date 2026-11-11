import type { SizeValue, SpaceTotal } from "../../ipc";
import { SIZE_UNIT_RULES } from "../../ipc";
import { formatByteUnit } from "../../i18n/bytes";

export type SizeDisplay = {
  system: "iec" | "si";
  unit: number | "auto";
};

export const windowsSizeDisplay: SizeDisplay = { system: "iec", unit: "auto" };

export type SizeBasisFilter = SizeValue["basis"];
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

const spaceReasonLabels = new Map([
  ["space.aux_not_included", "Auxiliary data was not collected"],
  ["space.dedup_capacity", "The file identity limit was reached"],
  ["space.sharing_not_collected", "Sharing evidence was not collected"],
  ["space.sharing_unknown", "Sharing could not be verified"],
  ["space.identity_unsupported", "File identity could not be verified"],
  ["space.out_of_range_links", "Links outside the scan were reported"],
  ["space.shared_reported", "The file system reported shared space"],
]);

const measurementLimitLabels = new Map([
  ["space.data_insufficient", "Some space data could not be verified"],
  [
    "space.aggregation_overflow",
    "The space total exceeded the supported range",
  ],
  ["space.allocated_not_collected", "Allocation data was not collected"],
  [
    "space.observation_detail_insufficient",
    "Some observation details are unavailable",
  ],
]);

/** Returns the explanation for a recognized item-space reason. */
export function displaySpaceReason(key: string): string | undefined {
  return spaceReasonLabels.get(key);
}

/** Preserves a visible limitation even when its reason is not recognized. */
export function displayMeasurementLimit(key: string): string {
  return (
    displaySpaceReason(key) ??
    measurementLimitLabels.get(key) ??
    "A measurement limitation was reported"
  );
}

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

/** Retains the lower-bound qualifier when showing original bytes. */
export function displayRawSize(size: SizeValue): string | undefined {
  if (size.state === "unknown") return undefined;
  const rawBytes = formatBytes(size.bytes);
  return size.state === "lower_bound" ? `At least ${rawBytes}` : rawBytes;
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

/** Shows the original byte count in details without upgrading a lower bound. */
export function displaySizeWithBytes(
  size: SizeValue,
  display: SizeDisplay,
): string {
  const formatted = displaySize(size, display);
  const raw = displayRawSize(size);
  return !raw || formatted === raw ? formatted : `${formatted} · ${raw}`;
}
