import type { SizeValue, SpaceTotal } from "../../ipc";
import { SIZE_UNIT_RULES } from "../../ipc";
import { formatByteUnit } from "../../i18n/bytes";
import { displayInteger } from "../../i18n/regional-format";
import type { RegionalFormat } from "../../i18n/regional-format";
import type { LocaleText } from "../../i18n/presentation";

import type { SizeDisplay } from "../../i18n/bytes";
export type { SizeDisplay } from "../../i18n/bytes";

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
const evidenceLabels = {
  measured: "Measured",
  os_reported: "Reported by the file system",
  estimated: "Estimated",
  unknown: undefined,
} satisfies Record<SizeValue["evidence"], string | undefined>;

type Message = readonly [key: string, fallback: string];

/** The message of each item-space reason, by its reason key. */
const spaceReasonMessages = new Map<string, Message>([
  [
    "space.aux_not_included",
    ["ui.analysis.space_aux_not_included", "Auxiliary data was not collected"],
  ],
  [
    "space.dedup_capacity",
    [
      "ui.analysis.space_dedup_capacity",
      "There were too many files that may share content, so some weren't counted. Allocated and unique physical sizes are minimums.",
    ],
  ],
  [
    "space.sharing_not_collected",
    [
      "ui.analysis.space_sharing_not_collected",
      "Sharing evidence was not collected",
    ],
  ],
  [
    "space.sharing_unknown",
    ["ui.analysis.space_sharing_unknown", "Sharing could not be verified"],
  ],
  [
    "space.identity_unsupported",
    [
      "ui.analysis.space_identity_unsupported",
      "File identity could not be verified",
    ],
  ],
  [
    "space.out_of_range_links",
    [
      "ui.analysis.space_out_of_range_links",
      "Links outside the analysis scope were reported",
    ],
  ],
  [
    "space.shared_reported",
    [
      "ui.analysis.space_shared_reported",
      "The file system reported shared space",
    ],
  ],
  [
    "space.placeholder_local_unknown",
    [
      "ui.analysis.space_placeholder_local_unknown",
      "This is a cloud placeholder, and the space it uses on this device was not reported",
    ],
  ],
]);

/** The message of each reason that only limits a measurement. */
const measurementLimitMessages = new Map<string, Message>([
  [
    "space.data_insufficient",
    [
      "ui.analysis.space_data_insufficient",
      "Some space data could not be verified",
    ],
  ],
  [
    "space.aggregation_overflow",
    [
      "ui.analysis.space_aggregation_overflow",
      "The space total exceeded the supported range",
    ],
  ],
  [
    "space.allocated_not_collected",
    [
      "ui.analysis.space_allocated_not_collected",
      "Allocation data was not collected",
    ],
  ],
  [
    "space.observation_detail_insufficient",
    [
      "ui.analysis.space_observation_detail_insufficient",
      "Some observation details are unavailable",
    ],
  ],
  [
    "space.hardlink_identity_unknown",
    [
      "ui.analysis.space_hardlink_identity_unknown",
      "The allocated size of files that could not be identified was left out of the total.",
    ],
  ],
]);

const unrecognizedLimit: Message = [
  "ui.analysis.space_measurement_limit_reported",
  "A measurement limitation was reported",
];

/**
 * Explains a recognized item-space reason in the active language. An
 * unrecognized key gives no text, so the caller decides what it shows then.
 */
export function displaySpaceReason(
  key: string,
  text: LocaleText,
): string | undefined {
  const message = spaceReasonMessages.get(key);
  return message && text(...message);
}

/**
 * Explains a measurement limit in the active language. An unrecognized key
 * still shows that a limitation was reported, without naming it.
 */
export function displayMeasurementLimit(key: string, text: LocaleText): string {
  return text(
    ...(spaceReasonMessages.get(key) ??
      measurementLimitMessages.get(key) ??
      unrecognizedLimit),
  );
}

/**
 * The measurement limits that every size row shares, said once below the rows
 * (#214). The common pair of missing auxiliary data and the observation
 * details it leaves out reads as one sentence.
 */
export function displaySharedLimits(
  keys: readonly string[],
  text: LocaleText,
): string {
  if (
    keys.length === 2 &&
    keys.includes("space.aux_not_included") &&
    keys.includes("space.observation_detail_insufficient")
  ) {
    return text(
      "ui.analysis.space_aux_detail_unavailable",
      "Auxiliary data was not collected, so some observation details are unavailable.",
    );
  }
  return keys.map((key) => displayMeasurementLimit(key, text)).join(" ");
}

/** Explains a space reason of either table; a key without a text gives none. */
export function displayKnownSpaceReason(
  key: string,
  text: LocaleText,
): string | undefined {
  const message =
    spaceReasonMessages.get(key) ?? measurementLimitMessages.get(key);
  return message && text(...message);
}

/** Names a unit system by what its units look like. */
export function displayUnitSystem(
  system: SizeDisplay["system"],
  text: LocaleText,
): string {
  return system === "iec"
    ? text("ui.analysis.unit_system_binary", "Binary units (KiB, MiB)")
    : text("ui.analysis.unit_system_decimal", "Decimal units (kB, MB)");
}

/** How the scan stands that is still to supply a size without a value. */
export type SizeWait = "running" | "paused";

/**
 * Reasons that leave a size without a value for good. Every other reason on
 * an unknown size also comes with the first result of a scan, before a file
 * was read, so it does not tell whether the value is still to come.
 */
const lastingUnknownReasons = new Set([
  "space.aggregation_overflow",
  "space.dedup_capacity",
  "space.placeholder_local_unknown",
]);

/**
 * Tells whether a size without a value is still expected from the scan, given
 * how the scan stands (nothing once it ended). Only the logical and the
 * allocated size are awaited: they get a value as soon as the scan has read
 * a file, while the unique physical size can stay unknown for a whole scan.
 */
export function awaitedSize(
  size: SizeValue,
  measurementLimits: readonly string[],
  wait: SizeWait | undefined,
): SizeWait | undefined {
  if (size.state !== "unknown" || size.basis === "unique_physical")
    return undefined;
  return measurementLimits.some((key) => lastingUnknownReasons.has(key))
    ? undefined
    : wait;
}

const awaitedSizeMessages: Record<
  SizeWait,
  { value: Message; detail: Message }
> = {
  running: {
    value: ["ui.analysis.size_calculating", "Calculating…"],
    detail: ["ui.analysis.size_calculating_detail", "Checking file sizes"],
  },
  paused: {
    value: ["ui.analysis.size_no_value_yet", "No value yet"],
    detail: [
      "ui.analysis.size_no_value_yet_detail",
      "It appears when the scan continues",
    ],
  },
};

/** What stands in the place of an awaited size, and the line below it. */
export function displayAwaitedSize(
  wait: SizeWait,
  text: LocaleText,
): { value: string; detail: string } {
  const { value, detail } = awaitedSizeMessages[wait];
  return { value: text(...value), detail: text(...detail) };
}

/** Separates measurement completeness and evidence from the result version. */
export function displayMeasurement(
  value: SizeValue,
  completeness: SpaceTotal["completeness"],
  text?: LocaleText,
): string {
  return [
    text?.(
      `ui.analysis.measurement_${completeness}`,
      measurementLabels[completeness],
    ) ?? measurementLabels[completeness],
    displayEvidence(value.evidence, text),
  ]
    .filter(Boolean)
    .join(" · ");
}

/** Localizes a measurement source while retaining undefined for unknown evidence. */
export function displayEvidence(
  evidence: SizeValue["evidence"],
  text?: LocaleText,
): string | undefined {
  if (evidence === "unknown") return undefined;
  const key =
    evidence === "measured"
      ? "ui.analysis.evidence_measured"
      : evidence === "os_reported"
        ? "ui.analysis.evidence_reported"
        : "ui.analysis.evidence_estimated";
  return (
    text?.(key, evidenceLabels[evidence] ?? "Unavailable") ??
    evidenceLabels[evidence]
  );
}

/** Formats exact wire bytes without converting them to a floating-point number. */
export function formatBytes(
  bytes: string,
  regional?: RegionalFormat,
  text?: LocaleText,
): string {
  const byteUnit =
    regional?.byteUnitText ??
    text?.("ui.analysis.bytes_unit", "bytes") ??
    "bytes";
  return `${displayInteger(BigInt(bytes), regional)} ${byteUnit}`;
}

/** Retains the lower-bound qualifier when showing original bytes. */
export function displayRawSize(
  size: SizeValue,
  regional?: RegionalFormat,
  text?: LocaleText,
): string | undefined {
  if (size.state === "unknown") return undefined;
  const rawBytes = formatBytes(size.bytes, regional, text);
  return size.state === "lower_bound"
    ? lowerBoundLabel(rawBytes, text)
    : rawBytes;
}

function lowerBoundLabel(value: string, text?: LocaleText) {
  return (
    text?.("ui.analysis.size_lower_bound", "At least {value}", { value }) ??
    `At least ${value}`
  );
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
    display.regional,
  );
}

/** Preserves a lower bound or missing observation in the displayed size. */
export function displaySize(
  size: SizeValue,
  display: SizeDisplay = windowsSizeDisplay,
  text?: LocaleText,
): string {
  if (size.state === "unknown")
    return text?.("ui.analysis.unavailable", "Unavailable") ?? "Unavailable";
  const lowerBound = size.state === "lower_bound";
  const formatted = formatSizeBytes(size.bytes, display, lowerBound);
  return lowerBound ? lowerBoundLabel(formatted, text) : formatted;
}

/** Returns a separate raw count only when it adds information. */
export function displaySizeParts(
  size: SizeValue,
  display: SizeDisplay,
  text?: LocaleText,
) {
  const formatted = displaySize(size, display, text);
  const raw = displayRawSize(size, display.regional, text);
  return { formatted, raw: raw === formatted ? undefined : raw };
}

/** Shows the original byte count in details without upgrading a lower bound. */
export function displaySizeWithBytes(
  size: SizeValue,
  display: SizeDisplay,
  text?: LocaleText,
): string {
  const { formatted, raw } = displaySizeParts(size, display, text);
  return raw ? `${formatted} · ${raw}` : formatted;
}
