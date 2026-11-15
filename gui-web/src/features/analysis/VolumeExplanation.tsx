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
import { AnalysisError } from "../../ipc";
import type { AnalysisClient, OsPath, VolumeGapResponse } from "../../ipc";
import { createReflectCadence } from "../../state/result-cadence";
import type { ReflectCadence } from "../../state/result-cadence";
import {
  displayMeasurement,
  displaySizeWithBytes,
  displayEvidence,
} from "./space-format";
import type { SizeDisplay } from "./space-format";
import {
  amount,
  factorLabels,
  lookupProblem,
  observationRows,
  volumeBytes,
} from "./volume-format";
import type { Message } from "./volume-format";
import { useHeldValue } from "./use-selection-hold";

/**
 * The shortest spacing between two reads of the whole-volume comparison
 * while it stays open (F02 design review of PR #69, item 1). The comparison
 * is read on its own, so it follows the summary at half its pace.
 */
export const VOLUME_COMPARE_REREAD_MS = 2000;

type Target = { root: number } | { volumeId: string };

function isWholeVolume(target: Target | undefined): boolean {
  return target !== undefined && "volumeId" in target;
}

/** What was read for one target from one result version. */
type Outcome = {
  target: string;
  version: string;
  response?: VolumeGapResponse;
  problem?: Message;
};

/**
 * Keeps the explanation of `target` on `version`, the result the summary
 * shows.
 *
 * Reads never overlap: a version that arrives while a read is in flight is
 * read when that read ends, and only the newest one is read. While a newer
 * read is in flight the previous explanation stays in place, so the section
 * keeps its height and expanded parts. The whole-volume comparison is read
 * at most once per `VOLUME_COMPARE_REREAD_MS`. A reread refused because the
 * result was replaced keeps what is shown until the summary moves on.
 */
function useVolumeReport(
  client: AnalysisClient,
  version: string,
  target: Target | undefined,
) {
  const [outcome, setOutcome] = useState<Outcome>();
  const [reading, setReading] = useState(false);
  const [wake, setWake] = useState(0);
  const sequence = useRef(0);
  const refusedAt = useRef<string>(undefined);
  const pace = useRef<ReflectCadence>(undefined);
  const key = target && JSON.stringify(target);
  const current = outcome && outcome.target === key ? outcome : undefined;
  useEffect(() => () => pace.current?.dispose(), []);
  useEffect(() => {
    if (!target || key === undefined || reading) return;
    const shown = current !== undefined;
    if (current?.version === version) return;
    if (shown && refusedAt.current === version) return;
    if (isWholeVolume(target)) {
      pace.current ??= createReflectCadence(VOLUME_COMPARE_REREAD_MS);
      if (shown && pace.current.remaining() > 0) {
        pace.current.defer(() => setWake((count) => count + 1));
        return;
      }
      pace.current.mark();
    }
    const requestSeq = ++sequence.current;
    const selection =
      "root" in target
        ? { root: { resultVersion: version, itemNo: target.root } }
        : { volumeId: target.volumeId };
    setReading(true);
    client
      .call("space.volume_gap", {
        resultVersion: version,
        requestSeq,
        target: selection,
      })
      .then(
        (response) => {
          setOutcome(
            response.resultVersion !== version ||
              response.requestSeq !== requestSeq
              ? {
                  target: key,
                  version,
                  problem: [
                    "ui.analysis.volume_response_mismatch",
                    "The response does not match this scan result.",
                  ],
                }
              : { target: key, version, response },
          );
        },
        (failure: unknown) => {
          if (
            shown &&
            failure instanceof AnalysisError &&
            failure.code === "STALE_VERSION"
          ) {
            refusedAt.current = version;
            return;
          }
          setOutcome({ target: key, version, problem: lookupProblem(failure) });
        },
      )
      .finally(() => setReading(false));
    // The key names the target; the client and target are read with it.
  }, [key, version, current, reading, wake]);
  return {
    current,
    pending: key !== undefined && current === undefined,
    /** Forgets what was read so the next open reads again. */
    reset: () => setOutcome(undefined),
  };
}

/**
 * Returns what to show for `current`: while `hold` is on, what was shown
 * when it began. Closing or changing the target is never held back.
 */
function useShownReport(current: Outcome | undefined, hold: boolean) {
  const held = useHeldValue(current, hold);
  const shown = held && held.target === current?.target ? held : current;
  return { report: shown?.response, problem: shown?.problem };
}

/**
 * Explains volume usage as an expanded part of the summary.
 *
 * `resultVersion` is the result the summary shows. While open, the
 * explanation follows it, so the summary never shows values from two results
 * one above the other; the summary already paces its versions and holds them
 * while text is selected. `hold` is that selection hold: an explanation
 * read while it is on is shown once the hold ends.
 */
export function VolumeExplanation({
  client,
  resultVersion,
  roots,
  display,
  hold = false,
}: {
  client: AnalysisClient;
  resultVersion: string;
  roots: readonly OsPath[];
  display: SizeDisplay;
  hold?: boolean;
}) {
  const text = useLocaleText();
  const [root, setRoot] = useState("0");
  const [target, setTarget] = useState<Target>();
  const rootIndex = Number(root);
  const { current, pending, reset } = useVolumeReport(
    client,
    resultVersion,
    target,
  );
  const { report, problem } = useShownReport(current, hold);
  const wholeVolume = isWholeVolume(target);

  function close() {
    setTarget(undefined);
    reset();
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
            reset();
            setTarget({ root: rootIndex });
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
      {report && (
        <>
          <Readout
            label={text("ui.analysis.volume_summary", "Volume usage summary")}
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
