import { Choice, LineChoice } from "../../components";
import type { AnalysisTarget, TargetProbeEvent } from "../../ipc";
import type { useLocaleText } from "../../i18n/presentation";
import type { useRegionalFormat } from "../../i18n/regional-format";
import { describeTargetCapacity } from "./target-capacity";
import { accessLabel } from "./target-access";
import type { ShownAccess } from "./target-access";

function kindLabel(
  kind: AnalysisTarget["kind"] | undefined,
  text: ReturnType<typeof useLocaleText>,
): string {
  if (!kind)
    return text("ui.analysis.target_type_unavailable", "Type unavailable");
  return (
    {
      internal: text("ui.analysis.target_internal", "internal"),
      external: text("ui.analysis.target_external", "external"),
      network_mount: text("ui.analysis.target_network_mount", "network mount"),
      folder_mount: text("ui.analysis.target_folder_mount", "folder mount"),
    }[kind] ?? text("ui.common.unknown", "Unknown")
  );
}

/**
 * One drive of the picker. A target in the default list is a card with its capacity;
 * a target outside it, shown on request, stands on one line with its path cut in the
 * middle (F01 #263, F02 #350).
 */
export function DriveChoice({
  target,
  probe,
  access,
  deferred,
  checkingCapacity,
  showSummary,
  startupDiskLabel,
  text,
  regional,
}: {
  target: AnalysisTarget;
  probe: TargetProbeEvent | undefined;
  access: ShownAccess;
  deferred: boolean;
  checkingCapacity: boolean;
  showSummary: boolean;
  startupDiskLabel: string;
  text: ReturnType<typeof useLocaleText>;
  regional: ReturnType<typeof useRegionalFormat>;
}) {
  const capacity = describeTargetCapacity(
    probe,
    checkingCapacity,
    text,
    regional,
  );
  const isStartupDisk = target.isStartupVolume === true;
  const label = isStartupDisk
    ? (target.volumeName ?? startupDiskLabel)
    : (target.volumeName ?? target.path.display);
  const common = {
    value: target.path.raw,
    label,
    ...(isStartupDisk
      ? {
          accessibleName: `${label}, ${target.path.display}`,
          title: `${label} · ${target.path.display}${
            label === startupDiskLabel ? "" : ` · ${startupDiskLabel}`
          }`,
        }
      : {}),
    icon: "drive" as const,
    description: [
      ...(isStartupDisk ? [startupDiskLabel] : []),
      kindLabel(target.kind, text),
      capacity.evidence,
      ...(deferred
        ? [
            text(
              "ui.analysis.target_network_checked_on_select",
              "Network volumes are checked when you select them.",
            ),
          ]
        : []),
    ].join(" · "),
    status: {
      text: accessLabel(access, text),
      tone: access === "accessible" ? ("good" as const) : ("neutral" as const),
      shape: access === "checking" ? ("progress" as const) : undefined,
    },
  };
  if (target.defaultVisible === false) return <LineChoice {...common} />;
  return (
    <Choice
      {...common}
      summary={showSummary ? capacity.summary : undefined}
      availablePercent={capacity.availablePercent}
    />
  );
}
