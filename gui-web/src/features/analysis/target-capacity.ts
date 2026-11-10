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

/** Describe reported values and their limits without inferring unobserved data. */
export function describeTargetCapacity(
  observation: TargetProbeEvent | undefined,
  checking: boolean,
): string {
  if (!observation && checking) return "Checking capacity…";
  const total = observation?.totalBytes ?? null;
  const available = observation?.availableBytes ?? null;
  const parts = [
    `Total: ${capacityValue(total)}`,
    `Available: ${capacityValue(available)}`,
  ];
  if (!observation)
    return [...parts, "No capacity check was reported."].join(" · ");

  if (total === null || available === null)
    parts.push(
      capacityReasons.get(observation.capacityReason ?? "") ??
        "No explanation is available for the unverified capacity.",
    );
  if (observation.aggregationUncertain)
    parts.push("Combined capacity unverified. Paths may share a volume.");
  parts.push(`Checked: ${observation.observedAt}`);
  return parts.join(" · ");
}
