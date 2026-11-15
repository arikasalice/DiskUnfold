import { useState } from "react";
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
  factorLabels,
  observationRows,
  volumeBytes,
} from "./volume-format";
import { isWholeVolume } from "./summary-batch";
import type { VolumePart } from "./summary-batch";

/**
 * Explains volume usage as an expanded part of the summary.
 *
 * It shows what `volume` holds for the result the summary shows; the summary
 * batch changes the two together (see `createSummaryBatch`). While the
 * summary has moved on without it, the part clears its values and says it is
 * updating, keeping its height, its expanded parts and focus. That line is
 * not announced: values that change during a scan are not read out.
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
  const { view, target } = volume;
  const rootIndex = Number(root);
  const report = view.state === "shown" ? view.response : undefined;
  const problem = view.state === "shown" ? view.problem : undefined;
  const updating = view.state === "updating";
  const volumeId = updating ? view.volumeId : (report?.volumeId ?? null);
  const wholeVolume = isWholeVolume(target);

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
            volume.close();
            setRoot(selected);
          }}
        />
      )}
      <Action
        isDisabled={view.state === "reading"}
        onPress={() => {
          if (report || updating) volume.close();
          else volume.open({ root: rootIndex });
        }}
      >
        {report || updating
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
      {problem && <Notice tone="error">{text(...problem)}</Notice>}
      {(report || updating) && (
        <HeldHeight holding={updating}>
          {report ? (
            <ReportSummary
              report={report}
              wholeVolume={wholeVolume}
              display={display}
            />
          ) : (
            <Text kind="muted">
              {text("ui.analysis.volume_updating", "Updating…")}
            </Text>
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
          {!wholeVolume && roots.length > 1 && volumeId && (
            <Action onPress={() => volume.open({ volumeId })}>
              {text(
                "ui.analysis.volume_compare_all",
                "Compare all scanned folders on this volume",
              )}
            </Action>
          )}
        </HeldHeight>
      )}
    </section>
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
                  ? text("ui.analysis.volume_none_reported", "None reported")
                  : report.osCategories.rows
                      .map(
                        ({ key, value }) =>
                          `${key} · ${amount(value, display, text)}`,
                      )
                      .join(", "),
          },
        ]}
      />
    </>
  );
}
