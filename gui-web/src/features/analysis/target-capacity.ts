import { formatByteUnit } from "../../i18n/bytes";
import { SIZE_UNIT_RULES } from "../../ipc";
import type { TargetProbeEvent } from "../../ipc";
import { formatBytes } from "./space-format";

const capacityReasons = new Map([
  ["os.unsupported", "Capacity reporting is unavailable for this target."],
  ["os.query_failed", "The capacity check failed."],
  ["os.call_slow", "The capacity check did not finish in time."],
  ["scan.path_denied", "Access to this target was denied."],
  ["scan.path_vanished", "The path was no longer available."],
  ["scan.disconnected", "The target was disconnected."],
  ["path.unresolvable", "The path could not be resolved."],
  ["job.cancelled", "The capacity check was cancelled."],
]);

function capacityValue(bytes: string | null): string {
  return bytes === null ? "Could not verify" : formatBytes(bytes);
}

function summaryValue(bytes: string): string {
  return formatByteUnit(
    BigInt(bytes),
    SIZE_UNIT_RULES.iec,
    SIZE_UNIT_RULES.decimalPlaces,
    "auto",
    false,
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
): {
  summary: string;
  evidence: string;
  availablePercent?: number | undefined;
} {
  if (!observation && checking)
    return {
      summary: "Checking drive capacity…",
      evidence: "Checking capacity…",
    };
  const total = observation?.totalBytes ?? null;
  const available = observation?.availableBytes ?? null;
  const summary =
    total === null && available === null
      ? "Capacity unavailable"
      : [
          available === null
            ? "Available unverified"
            : "Available " + summaryValue(available),
          total === null ? "Total unverified" : "Total " + summaryValue(total),
        ].join(" · ");
  const parts = [
    `Total: ${capacityValue(total)}`,
    `Available: ${capacityValue(available)}`,
  ];
  if (!observation)
    return {
      summary,
      evidence: [...parts, "No capacity check was reported."].join(" · "),
    };

  if (total === null || available === null)
    parts.push(
      capacityReasons.get(observation.capacityReason ?? "") ??
        "No explanation is available for the unverified capacity.",
    );
  if (observation.aggregationUncertain)
    parts.push("Combined capacity unverified. Paths may share a volume.");
  parts.push(`Checked: ${observation.observedAt}`);
  return {
    summary,
    evidence: parts.join(" · "),
    availablePercent: availablePercent(total, available),
  };
}
