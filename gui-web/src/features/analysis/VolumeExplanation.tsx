import { useEffect, useRef, useState } from "react";
import {
  Action,
  DetailDisclosure,
  Notice,
  Progress,
  Readout,
  SelectField,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import { useTimestampText } from "../../i18n/regional-format";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  OsPath,
  VolumeGapAmount,
  VolumeGapObservation,
  VolumeGapResponse,
} from "../../ipc";
import {
  displayMeasurement,
  displaySizeWithBytes,
  displayEvidence,
  formatBytes,
  formatSizeBytes,
} from "./space-format";
import type { SizeDisplay } from "./space-format";
import { useDialogResult } from "./use-dialog-result";
import { DialogUpdateStatus } from "./DialogUpdateStatus";

type Message = readonly [key: string, fallback: string];
const factorLabels: Readonly<Record<string, Message>> = {
  "gap.out_of_scope": [
    "ui.analysis.volume_factor_out_of_scope",
    "Outside the scan scope",
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
function volumeBytes(
  bytes: string,
  display: SizeDisplay,
  text: ReturnType<typeof useLocaleText>,
): string {
  const converted = formatSizeBytes(bytes, display);
  const raw = formatBytes(bytes, display.regional, text);
  return converted === raw ? raw : `${converted} · ${raw}`;
}

function amount(
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

function observationRows(
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
function lookupProblem(failure: unknown): Message {
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
            "The selected target is outside this scan. Choose a folder or volume included in the scan.",
          ];
    }
  }
  return [
    "ui.analysis.volume_unavailable",
    "The volume explanation is unavailable.",
  ];
}

type Target = { root: number } | { volumeId: string };

function isWholeVolume(target: Target | undefined): boolean {
  return target !== undefined && "volumeId" in target;
}

type Outcome = {
  key: string;
  response?: VolumeGapResponse;
  published?: string | undefined;
  problem?: Message;
};

/** Reads the result's publication time; a failure leaves it unknown. */
function readPublished(
  client: AnalysisClient,
  resultVersion: string,
  itemNo: number,
): Promise<string | undefined> {
  return client
    .call("browse.detail", {
      resultVersion,
      item: { resultVersion, itemNo },
    })
    .then(({ observedAt }) =>
      observedAt.state === "present" ? observedAt.value : undefined,
    )
    .catch(() => undefined);
}

/**
 * Reads the explanation of `target` from `version` and its publication time.
 * A read refused because the result was replaced is reported to `stale`.
 */
function useVolumeReport(
  client: AnalysisClient,
  version: string | undefined,
  target: Target | undefined,
  rootIndex: number,
  stale: (version: string) => void,
) {
  const [outcome, setOutcome] = useState<Outcome>();
  const sequence = useRef(0);
  const key =
    target && version !== undefined
      ? JSON.stringify([version, target])
      : undefined;
  useEffect(() => {
    if (key === undefined || !target || version === undefined) return;
    let active = true;
    const requestSeq = ++sequence.current;
    const selection =
      "root" in target
        ? { root: { resultVersion: version, itemNo: target.root } }
        : { volumeId: target.volumeId };
    const published = readPublished(
      client,
      version,
      "root" in target ? target.root : rootIndex,
    );
    client
      .call("space.volume_gap", {
        resultVersion: version,
        requestSeq,
        target: selection,
      })
      .then(
        async (response) => {
          const publishedAt = await published;
          if (!active) return;
          setOutcome(
            response.resultVersion !== version ||
              response.requestSeq !== requestSeq
              ? {
                  key,
                  problem: [
                    "ui.analysis.volume_response_mismatch",
                    "The response does not match this scan result.",
                  ],
                }
              : { key, response, published: publishedAt },
          );
        },
        (failure: unknown) => {
          if (!active) return;
          setOutcome({ key, problem: lookupProblem(failure) });
          if (
            failure instanceof AnalysisError &&
            failure.code === "STALE_VERSION"
          )
            stale(version);
        },
      );
    return () => {
      active = false;
    };
    // The key names the version and target; the rest only describes them.
  }, [key]);
  const current = outcome && outcome.key === key ? outcome : undefined;
  return { current, pending: key !== undefined && current === undefined };
}

function publishedRow(
  outcome: Outcome | undefined,
  text: ReturnType<typeof useLocaleText>,
  timestamp: ReturnType<typeof useTimestampText>,
) {
  const published = outcome?.published;
  return {
    label: text("ui.inspection.published", "Result published"),
    value: published
      ? timestamp(Date.parse(published), () => published)
      : text("ui.analysis.volume_time_unavailable", "Time unavailable"),
  };
}

/**
 * Explains volume usage for one result, like a dialog: it keeps what it read
 * when it opened and shows when that result was published. Comparing all
 * folders on the volume reads the newest result again when the opened one
 * has been replaced, and says so once. `resultVersion` is the newest result
 * known for the scan.
 */
export function VolumeExplanation({
  client,
  resultVersion,
  roots,
  display,
}: {
  client: AnalysisClient;
  resultVersion: string;
  roots: readonly OsPath[];
  display: SizeDisplay;
}) {
  const text = useLocaleText();
  const timestamp = useTimestampText();
  const dialog = useDialogResult(resultVersion);
  const [root, setRoot] = useState("0");
  const [target, setTarget] = useState<Target>();
  const rootIndex = Number(root);
  const { current, pending } = useVolumeReport(
    client,
    dialog.version,
    target,
    rootIndex,
    dialog.stale,
  );
  const report = current?.response;
  const problem = current?.problem;
  const wholeVolume = isWholeVolume(target);

  function close() {
    setTarget(undefined);
    dialog.close();
  }

  if (roots.length === 0) return null;
  // The admitted result keeps root items in the reviewed canonical order.
  const options = roots.map((path, index) => ({
    value: String(index),
    label: path.display,
  }));

  return (
    <section
      aria-label={text("ui.analysis.volume_section", "Volume explanation")}
    >
      {roots.length > 1 && (
        <SelectField
          label={text("ui.analysis.volume_scanned_folder", "Scanned folder")}
          value={root}
          options={options}
          onChange={(selected) => {
            close();
            setRoot(selected);
          }}
        />
      )}
      <Action
        isDisabled={pending}
        onPress={() => {
          if (report) {
            close();
          } else {
            setTarget({ root: rootIndex });
            dialog.open();
          }
        }}
      >
        {report
          ? text("ui.analysis.volume_hide", "Hide volume explanation")
          : text("ui.analysis.volume_explain", "Explain volume usage")}
      </Action>
      {pending && (
        <Progress
          label={text(
            "ui.analysis.volume_reading",
            "Reading volume observations",
          )}
        />
      )}
      {problem && <Notice tone="error">{text(...problem)}</Notice>}
      <DialogUpdateStatus updated={dialog.updated && report !== undefined} />
      {report && (
        <>
          <Readout
            label={text("ui.analysis.volume_summary", "Volume usage summary")}
            rows={[
              publishedRow(current, text, timestamp),
              {
                label: text(
                  "ui.analysis.volume_compared_scope",
                  "Compared scope",
                ),
                value: wholeVolume
                  ? text(
                      "ui.analysis.volume_all_scanned_folders",
                      "All scanned folders on this volume",
                    )
                  : text(
                      "ui.analysis.volume_selected_folder",
                      "Selected scanned folder",
                    ),
              },
              {
                label: text(
                  "ui.analysis.volume_used_end",
                  "Volume used at end of scan",
                ),
                value:
                  report.end.state === "available"
                    ? text(
                        "ui.analysis.volume_end_value",
                        "{used} · {evidence} · {time}",
                        {
                          used: volumeBytes(
                            report.end.usedBytes,
                            display,
                            text,
                          ),
                          evidence:
                            displayEvidence(report.end.evidence, text) ??
                            text(
                              "ui.analysis.volume_evidence_unavailable",
                              "Evidence unavailable",
                            ),
                          time: report.end.observedAt
                            ? text("ui.analysis.volume_time", "{time}", {
                                time: report.end.observedAt,
                              })
                            : text(
                                "ui.analysis.volume_time_unavailable",
                                "Time unavailable",
                              ),
                        },
                      )
                    : text("ui.analysis.unavailable", "Unavailable"),
              },
              {
                label: text(
                  "ui.analysis.volume_scanned_allocation",
                  "Scanned allocation",
                ),
                value: `${displaySizeWithBytes(report.allocated.size, display, text)} · ${displayMeasurement(report.allocated.size, report.allocated.completeness, text)}`,
              },
              {
                label: text(
                  "ui.analysis.volume_unexplained_difference",
                  "Unexplained difference",
                ),
                value: amount(report.remainder, display, text),
              },
            ]}
          />
          <DetailDisclosure
            title={text(
              "ui.analysis.volume_details_title",
              "Observation and difference details",
            )}
          >
            <Readout
              label={text(
                "ui.analysis.volume_observations",
                "Volume observations",
              )}
              rows={[
                ...observationRows("start", report.start, display, text),
                ...observationRows("end", report.end, display, text),
              ]}
            />
            <Readout
              label={text("ui.analysis.volume_difference", "Volume difference")}
              rows={[
                {
                  label: text(
                    "ui.analysis.volume_compared_scope",
                    "Compared scope",
                  ),
                  value: wholeVolume
                    ? text(
                        "ui.analysis.volume_all_scanned_folders",
                        "All scanned folders on this volume",
                      )
                    : text(
                        "ui.analysis.volume_selected_folder",
                        "Selected scanned folder",
                      ),
                },
                {
                  label: text(
                    "ui.analysis.volume_scoped_allocation",
                    "Scoped allocated size",
                  ),
                  value: `${displaySizeWithBytes(report.allocated.size, display, text)} · ${displayMeasurement(report.allocated.size, report.allocated.completeness, text)}`,
                },
                {
                  label: text(
                    "ui.analysis.volume_scoped_unique_physical",
                    "Scoped unique physical size",
                  ),
                  value: `${displaySizeWithBytes(report.uniquePhysical.size, display, text)} · ${displayMeasurement(report.uniquePhysical.size, report.uniquePhysical.completeness, text)}`,
                },
                ...report.factors.map(({ key, value }) => ({
                  label: factorLabels[key] ? text(...factorLabels[key]) : key,
                  value: amount(value, display, text),
                })),
                {
                  label: text(
                    "ui.analysis.volume_unexplained_difference",
                    "Unexplained difference",
                  ),
                  value: amount(report.remainder, display, text),
                },
                {
                  label: text(
                    "ui.analysis.volume_os_system_categories",
                    "OS system categories",
                  ),
                  value:
                    report.osCategories.state === "unavailable"
                      ? text("ui.analysis.unavailable", "Unavailable")
                      : report.osCategories.rows.length === 0
                        ? text(
                            "ui.analysis.volume_none_reported",
                            "None reported",
                          )
                        : report.osCategories.rows
                            .map(
                              ({ key, value }) =>
                                `${key} · ${amount(value, display, text)}`,
                            )
                            .join(", "),
                },
              ]}
            />
          </DetailDisclosure>
          {!wholeVolume && roots.length > 1 && report.volumeId && (
            <Action
              onPress={() => {
                if (!report.volumeId) return;
                setTarget({ volumeId: report.volumeId });
                dialog.advance();
              }}
            >
              {text(
                "ui.analysis.volume_compare_all",
                "Compare all scanned folders on this volume",
              )}
            </Action>
          )}
        </>
      )}
    </section>
  );
}
