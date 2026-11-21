import { useRef, useState } from "react";
import type { ReactNode } from "react";
import {
  Action,
  DetailDisclosure,
  HeldHeight,
  Notice,
  Progress,
  Readout,
  SelectField,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { OsPath, VolumeGapResponse } from "../../ipc";
import {
  displayMeasurement,
  displaySizeWithBytes,
  displayEvidence,
} from "./space-format";
import type { SizeDisplay } from "./space-format";
import {
  amount,
  factorLabel,
  observationRows,
  unavailableValue,
  volumeBytes,
} from "./volume-format";
import type { Message } from "./volume-format";
import { isWholeVolume } from "./summary-batch";
import type { VolumePart, VolumeView } from "./summary-batch";

/** The volume the part last showed values for, kept while it shows none. */
function framedVolumeId(view: VolumeView): string | null {
  if (view.state === "shown") return view.response.volumeId;
  return view.state === "updating" || view.state === "failed"
    ? view.volumeId
    : null;
}

/**
 * Explains volume usage as an expanded part of the summary.
 *
 * It shows what `volume` holds for the result the summary shows; the summary
 * batch changes the two together (see `createSummaryBatch`). While the
 * summary has moved on without it, the part clears its values and says it is
 * updating, keeping its height, its expanded parts and focus. That line is
 * not announced: values that change during a scan are not read out. When no
 * read is due after a failure, the part shows the problem and "Retry" in the
 * same frame instead.
 */
export function VolumeExplanation({
  volume,
  roots,
  display,
}: {
  volume: VolumePart;
  roots: readonly OsPath[];
  display: SizeDisplay;
}) {
  const text = useLocaleText();
  const [root, setRoot] = useState("0");
  const toggle = useRef<HTMLButtonElement>(null);
  const { view } = volume;
  const failed = view.state === "failed" ? view : undefined;
  const framed = failed ? failed.framed : view.state !== "reading";
  const opened = view.state !== "closed" && view.state !== "reading";

  if (roots.length === 0) return null;
  // The admitted result keeps root items in the reviewed canonical order.
  const options = roots.map((path, index) => ({
    value: String(index),
    label: path.display,
  }));
  const problem = failed && (
    <ReadProblem
      problem={failed.problem}
      onRetry={() => {
        // The pressed action goes away; focus stays in the part.
        toggle.current?.focus();
        volume.retry();
      }}
    />
  );

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
            volume.close();
            setRoot(selected);
          }}
        />
      )}
      <Action
        ref={toggle}
        isDisabled={view.state === "reading"}
        onPress={() => {
          if (opened) volume.close();
          else volume.open({ root: Number(root) });
        }}
      >
        {opened
          ? text("ui.analysis.volume_hide", "Hide volume explanation")
          : text("ui.analysis.volume_explain", "Explain volume usage")}
      </Action>
      {view.state === "reading" && (
        <Progress
          label={text(
            "ui.analysis.volume_reading",
            "Reading volume observations",
          )}
        />
      )}
      {opened && !framed && problem}
      {opened && framed && (
        <PartFrame
          volume={volume}
          problem={problem}
          comparable={roots.length > 1}
          display={display}
        />
      )}
    </section>
  );
}

/** The problem of a failed read with the action that reads again. */
function ReadProblem({
  problem,
  onRetry,
}: {
  problem: Message;
  onRetry: () => void;
}) {
  const text = useLocaleText();
  return (
    <>
      <Notice tone="error">{text(...problem)}</Notice>
      <Action onPress={onRetry}>{text("ui.common.retry", "Retry")}</Action>
    </>
  );
}

/**
 * The part once it has shown values: the values, or in their place the
 * updating line or `problem`, above the details and the comparison action.
 * These stay mounted and the height is held, so what the user expanded or
 * focused survives a result the part could not read.
 */
function PartFrame({
  volume,
  problem,
  comparable,
  display,
}: {
  volume: VolumePart;
  problem: ReactNode;
  comparable: boolean;
  display: SizeDisplay;
}) {
  const text = useLocaleText();
  const { view } = volume;
  const report = view.state === "shown" ? view.response : undefined;
  const wholeVolume = isWholeVolume(volume.target);
  const volumeId = framedVolumeId(view);
  return (
    <HeldHeight holding={!report}>
      {report ? (
        <ReportSummary
          report={report}
          wholeVolume={wholeVolume}
          display={display}
        />
      ) : (
        (problem ?? (
          <Text kind="muted">
            {text("ui.analysis.volume_updating", "Updating…")}
          </Text>
        ))
      )}
      <DetailDisclosure
        title={text(
          "ui.analysis.volume_details_title",
          "Observation and difference details",
        )}
      >
        {report && (
          <ReportDetails
            report={report}
            wholeVolume={wholeVolume}
            display={display}
          />
        )}
      </DetailDisclosure>
      {!wholeVolume && comparable && volumeId && (
        <Action onPress={() => volume.open({ volumeId })}>
          {text(
            "ui.analysis.volume_compare_all",
            "Compare all scanned folders on this volume",
          )}
        </Action>
      )}
    </HeldHeight>
  );
}

type ReportProps = {
  report: VolumeGapResponse;
  wholeVolume: boolean;
  display: SizeDisplay;
};

/** The short summary of a volume report, shown first. */
function ReportSummary({ report, wholeVolume, display }: ReportProps) {
  const text = useLocaleText();
  return (
    <Readout
      label={text("ui.analysis.volume_summary", "Volume usage summary")}
      rows={[
        {
          label: text("ui.analysis.volume_compared_scope", "Compared scope"),
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
                    used: volumeBytes(report.end.usedBytes, display, text),
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
              : report.end.state === "unavailable"
                ? unavailableValue(report.end.reasonKey, text)
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
  );
}

/** Volume observations and the difference behind the summary. */
function ReportDetails({ report, wholeVolume, display }: ReportProps) {
  const text = useLocaleText();
  const otherVolumes = report.otherVolumes;
  const hasOtherVolumes = otherVolumes !== undefined;
  const otherVolumesTotal = hasOtherVolumes
    ? otherVolumesTotalText(otherVolumes ?? [], display, text)
    : undefined;
  return (
    <>
      <Readout
        label={text("ui.analysis.volume_observations", "Volume observations")}
        rows={[
          ...observationRows("start", report.start, display, text),
          ...observationRows("end", report.end, display, text),
        ]}
      />
      <Readout
        label={text("ui.analysis.volume_difference", "Volume difference")}
        rows={[
          {
            label: text("ui.analysis.volume_compared_scope", "Compared scope"),
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
          ...report.factors.flatMap(({ key, value }) =>
            key === "gap.out_of_scope" && hasOtherVolumes
              ? [
                  {
                    label: text(
                      "ui.analysis.volume_other_system_volumes",
                      "Other system volumes",
                    ),
                    value: otherVolumesTotal,
                  },
                  ...(otherVolumes ?? []).map(
                    ({ volumeName, role, amount: rowAmount }) => ({
                      label: otherVolumeLabel(volumeName, role, text),
                      value: amount(rowAmount, display, text),
                    }),
                  ),
                ]
              : [
                  {
                    label: factorLabel(key, text),
                    value: amount(value, display, text),
                  },
                ],
          ),
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
                ? unavailableValue(report.osCategories.reasonKey, text)
                : report.osCategories.rows.length === 0
                  ? text("ui.analysis.volume_none_reported", "None reported")
                  : report.osCategories.rows
                      .map(
                        // No category has a name yet, and a key is not shown.
                        ({ value }) =>
                          `${text("ui.common.unknown", "Unknown")} · ${amount(value, display, text)}`,
                      )
                      .join(", "),
          },
        ]}
      />
    </>
  );
}

function otherVolumesTotalText(
  rows: NonNullable<VolumeGapResponse["otherVolumes"]>,
  display: SizeDisplay,
  text: ReturnType<typeof useLocaleText>,
) {
  const unknown = rows.find((row) => row.amount.state === "unknown");
  if (unknown) return amount(unknown.amount, display, text);
  return volumeBytes(
    rows
      .reduce((total, row) => {
        if (row.amount.state !== "exact") return total;
        return total + BigInt(row.amount.bytes);
      }, 0n)
      .toString(),
    display,
    text,
  );
}

function otherVolumeLabel(
  volumeName: string,
  role: string | undefined,
  text: ReturnType<typeof useLocaleText>,
) {
  switch (role) {
    case "volume_role.vm":
      return text("ui.analysis.volume_role_vm", "Virtual memory (VM)");
    case "volume_role.preboot":
      return text(
        "ui.analysis.volume_role_preboot",
        "Startup preparation (Preboot)",
      );
    case "volume_role.recovery":
      return text("ui.analysis.volume_role_recovery", "Recovery");
    case "volume_role.update":
      return text("ui.analysis.volume_role_update", "Update");
    default:
      return volumeName;
  }
}
