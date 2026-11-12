import { formatByteUnit } from "../../i18n/bytes";
import type { RegionalFormat } from "../../i18n/regional-format";
import { interpolate } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { SIZE_UNIT_RULES } from "../../ipc";
import type { TargetProbeEvent } from "../../ipc";
import { formatBytes } from "./space-format";

type CapacityMessage = readonly [key: string, fallback: string];
const fallbackText: LocaleText = (key, fallback, values) =>
  interpolate(fallback || key, (name) => String(values?.[name] ?? "Unknown"));
const capacityReasons = new Map<string, CapacityMessage>([
  [
    "os.unsupported",
    [
      "ui.analysis.capacity_unsupported",
      "Capacity reporting is unavailable for this target.",
    ],
  ],
  [
    "os.query_failed",
    ["ui.analysis.capacity_failed", "The capacity check failed."],
  ],
  [
    "os.call_slow",
    [
      "ui.analysis.capacity_timed_out",
      "The capacity check did not finish in time.",
    ],
  ],
  [
    "scan.path_denied",
    ["ui.analysis.capacity_access_denied", "Access to this target was denied."],
  ],
  [
    "scan.path_vanished",
    ["ui.analysis.capacity_path_missing", "The path was no longer available."],
  ],
  [
    "scan.disconnected",
    ["ui.analysis.capacity_disconnected", "The target was disconnected."],
  ],
  [
    "path.unresolvable",
    ["ui.analysis.capacity_unresolvable", "The path could not be resolved."],
  ],
  [
    "job.cancelled",
    ["ui.analysis.capacity_cancelled", "The capacity check was cancelled."],
  ],
]);

function capacityValue(
  bytes: string | null,
  text: LocaleText,
  regional?: RegionalFormat,
): string {
  return bytes === null
    ? text("ui.analysis.capacity_could_not_verify", "Could not verify")
    : formatBytes(bytes, regional, text);
}

function capacityReason(
  reasonKey: string | null | undefined,
  text: LocaleText,
) {
  const message = capacityReasons.get(reasonKey ?? "");
  return message
    ? text(message[0], message[1])
    : text(
        "ui.analysis.capacity_no_explanation",
        "No explanation is available for the unverified capacity.",
      );
}

function summaryValue(bytes: string, regional?: RegionalFormat): string {
  return formatByteUnit(
    BigInt(bytes),
    SIZE_UNIT_RULES.iec,
    SIZE_UNIT_RULES.decimalPlaces,
    "auto",
    false,
    regional,
  );
}

function availablePercent(
  total: string | null,
  available: string | null,
): number | undefined {
  if (total === null || available === null) return undefined;
  const totalBytes = BigInt(total);
  const availableBytes = BigInt(available);
  if (totalBytes <= 0n || availableBytes < 0n || availableBytes > totalBytes)
    return undefined;
  return Number((availableBytes * 10_000n) / totalBytes) / 100;
}

/** Describe reported values and their limits without inferring unobserved data. */
export function describeTargetCapacity(
  observation: TargetProbeEvent | undefined,
  checking: boolean,
  text: LocaleText = fallbackText,
  regional?: RegionalFormat,
): {
  summary: string;
  evidence: string;
  availablePercent?: number | undefined;
} {
  if (!observation && checking)
    return {
      summary: text(
        "ui.analysis.capacity_checking_drive",
        "Checking drive capacity…",
      ),
      evidence: text("ui.analysis.capacity_checking", "Checking capacity…"),
    };
  const total = observation?.totalBytes ?? null;
  const available = observation?.availableBytes ?? null;
  const summary =
    total === null && available === null
      ? text("ui.analysis.capacity_unavailable", "Capacity unavailable")
      : [
          available === null
            ? text(
                "ui.analysis.capacity_available_unverified",
                "Available unverified",
              )
            : text("ui.analysis.capacity_available", "Available {value}", {
                value: summaryValue(available, regional),
              }),
          total === null
            ? text("ui.analysis.capacity_total_unverified", "Total unverified")
            : text("ui.analysis.capacity_total", "Total {value}", {
                value: summaryValue(total, regional),
              }),
        ].join(" · ");
  const parts = [
    text("ui.analysis.capacity_total_detail", "Total: {value}", {
      value: capacityValue(total, text, regional),
    }),
    text("ui.analysis.capacity_available_detail", "Available: {value}", {
      value: capacityValue(available, text, regional),
    }),
  ];
  if (!observation)
    return {
      summary,
      evidence: [
        ...parts,
        text(
          "ui.analysis.capacity_not_reported",
          "No capacity check was reported.",
        ),
      ].join(" · "),
    };

  if (total === null || available === null)
    parts.push(capacityReason(observation.capacityReason, text));
  if (observation.aggregationUncertain)
    parts.push(
      text(
        "ui.analysis.capacity_combined_unverified",
        "Combined capacity unverified. Paths may share a volume.",
      ),
    );
  parts.push(
    text("ui.analysis.capacity_checked", "Checked: {time}", {
      time: observation.observedAt,
    }),
  );
  return {
    summary,
    evidence: parts.join(" · "),
    availablePercent: availablePercent(total, available),
  };
}
