import type { useLocaleText } from "../../i18n/presentation";
import { AnalysisError } from "../../ipc";
import type { VolumeGapAmount, VolumeGapObservation } from "../../ipc";
import { displayEvidence, formatBytes, formatSizeBytes } from "./space-format";
import type { SizeDisplay } from "./space-format";

export type Message = readonly [key: string, fallback: string];
export const factorLabels: Readonly<Record<string, Message>> = {
  "gap.out_of_scope": [
    "ui.analysis.volume_factor_out_of_scope",
    "Outside the analysis scope",
  ],
  "gap.excluded": [
    "ui.analysis.volume_factor_excluded",
    "Excluded by scope rules",
  ],
  "gap.inaccessible": [
    "ui.analysis.volume_factor_inaccessible",
    "Inaccessible areas",
  ],
  "gap.dedup": ["ui.analysis.volume_factor_dedup", "Deduplicated allocation"],
  "gap.aux_not_collected": [
    "ui.analysis.volume_factor_auxiliary",
    "Uncollected auxiliary data",
  ],
  "gap.fs_metadata": [
    "ui.analysis.volume_factor_fs_metadata",
    "File system metadata",
  ],
  "gap.os_category": [
    "ui.analysis.volume_factor_os_category",
    "System categories",
  ],
  "gap.time_skew": [
    "ui.analysis.volume_factor_time_skew",
    "Observation timing",
  ],
};

/** Shows the selected unit and preserves the exact bytes in the explanation. */
export function volumeBytes(
  bytes: string,
  display: SizeDisplay,
  text: ReturnType<typeof useLocaleText>,
): string {
  const converted = formatSizeBytes(bytes, display);
  const raw = formatBytes(bytes, display.regional, text);
  return converted === raw ? raw : `${converted} · ${raw}`;
}

export function amount(
  value: VolumeGapAmount,
  display: SizeDisplay,
  text: ReturnType<typeof useLocaleText>,
): string {
  if (value.state === "unknown")
    return text("ui.analysis.unavailable", "Unavailable");
  return [
    volumeBytes(value.bytes, display, text),
    displayEvidence(value.evidence, text),
  ]
    .filter(Boolean)
    .join(" · ");
}

export function observationRows(
  point: "start" | "end",
  observation: VolumeGapObservation,
  display: SizeDisplay,
  text: ReturnType<typeof useLocaleText>,
) {
  const pointLabel =
    point === "start"
      ? text("ui.analysis.volume_start", "Start")
      : text("ui.analysis.volume_end", "End");
  const observationLabel = text(
    "ui.analysis.volume_observation",
    "{point} observation",
    { point: pointLabel },
  );
  if (observation.state === "not_attempted") {
    return [
      {
        label: observationLabel,
        value: text("ui.analysis.volume_not_attempted", "Not attempted"),
      },
    ];
  }
  const time = observation.observedAt
    ? text("ui.analysis.volume_time", "{time}", {
        time: observation.observedAt,
      })
    : text("ui.analysis.volume_time_unavailable", "Time unavailable");
  if (observation.state === "unavailable") {
    return [
      {
        label: observationLabel,
        value: text(
          "ui.analysis.volume_unavailable_at",
          "Unavailable · {time}",
          {
            time,
          },
        ),
      },
    ];
  }
  return [
    { label: observationLabel, value: time },
    {
      label: text(
        "ui.analysis.volume_capacity_label",
        "{point} volume capacity",
        { point: pointLabel },
      ),
      value: volumeBytes(observation.totalBytes, display, text),
    },
    {
      label: text("ui.analysis.volume_used_label", "{point} volume used", {
        point: pointLabel,
      }),
      value: volumeBytes(observation.usedBytes, display, text),
    },
    {
      label: text("ui.analysis.volume_free_label", "{point} volume free", {
        point: pointLabel,
      }),
      value: volumeBytes(observation.freeBytes, display, text),
    },
    {
      label: text("ui.analysis.volume_evidence_label", "{point} evidence", {
        point: pointLabel,
      }),
      value:
        displayEvidence(observation.evidence, text) ??
        text("ui.analysis.unavailable", "Unavailable"),
    },
  ];
}

/** Maps pinned volume lookup failures to their published recovery guidance. */
export function lookupProblem(failure: unknown): Message {
  if (failure instanceof AnalysisError) {
    switch (failure.code) {
      case "STALE_VERSION":
        return [
          "ui.analysis.volume_stale_result",
          "This result is no longer current. Open the latest result to explain its volume usage.",
        ];
      case "NOT_FOUND":
        return [
          "ui.analysis.volume_scan_unavailable",
          "This scan result is no longer available. Run the analysis again to explain its volume usage.",
        ];
      case "INVALID_ARGUMENT":
        if (failure.reasonKey === "space.target_out_of_scope")
          return [
            "ui.analysis.volume_target_outside_scan",
            "The selected target is outside this analysis scope. Choose a folder or volume included in the scope.",
          ];
    }
  }
  return [
    "ui.analysis.volume_unavailable",
    "The volume explanation is unavailable.",
  ];
}
