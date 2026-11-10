import type { SizeValue, SpaceTotal } from "../../ipc";

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

/** Preserves a lower bound or missing observation in the displayed size. */
export function displaySize(size: SizeValue): string {
  if (size.state === "unknown") return "Unavailable";
  return `${size.state === "lower_bound" ? "At least " : ""}${formatBytes(size.bytes)}`;
}
