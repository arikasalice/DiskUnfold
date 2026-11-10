import { useRef, useState } from "react";
import {
  Action,
  Notice,
  Progress,
  Readout,
  SelectField,
} from "../../components";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  OsPath,
  VolumeGapAmount,
  VolumeGapObservation,
  VolumeGapRequest,
  VolumeGapResponse,
} from "../../ipc";
import {
  displayMeasurement,
  displaySizeWithBytes,
  evidenceLabels,
  formatBytes,
  formatSizeBytes,
} from "./space-format";
import type { SizeDisplay } from "./space-format";

const factorLabels: Readonly<Record<string, string>> = {
  "gap.out_of_scope": "Outside the scan scope",
  "gap.excluded": "Excluded by scope rules",
  "gap.inaccessible": "Inaccessible areas",
  "gap.dedup": "Deduplicated allocation",
  "gap.aux_not_collected": "Uncollected auxiliary data",
  "gap.fs_metadata": "File system metadata",
  "gap.os_category": "System categories",
  "gap.time_skew": "Observation timing",
};

/** Shows the selected unit and preserves the exact bytes in the explanation. */
function volumeBytes(bytes: string, display: SizeDisplay): string {
  const converted = formatSizeBytes(bytes, display);
  const raw = formatBytes(bytes);
  return converted === raw ? raw : `${converted} · ${raw}`;
}

function amount(value: VolumeGapAmount, display: SizeDisplay): string {
  if (value.state === "unknown") return "Unavailable";
  return [volumeBytes(value.bytes, display), evidenceLabels[value.evidence]]
    .filter(Boolean)
    .join(" · ");
}

function observationRows(
  label: string,
  observation: VolumeGapObservation,
  display: SizeDisplay,
) {
  if (observation.state === "not_attempted") {
    return [{ label: `${label} observation`, value: "Not attempted" }];
  }
  const time = observation.observedAt ?? "Time unavailable";
  if (observation.state === "unavailable") {
    return [{ label: `${label} observation`, value: `Unavailable · ${time}` }];
  }
  return [
    { label: `${label} observation`, value: time },
    {
      label: `${label} volume capacity`,
      value: volumeBytes(observation.totalBytes, display),
    },
    {
      label: `${label} volume used`,
      value: volumeBytes(observation.usedBytes, display),
    },
    {
      label: `${label} volume free`,
      value: volumeBytes(observation.freeBytes, display),
    },
    {
      label: `${label} evidence`,
      value: evidenceLabels[observation.evidence] ?? "Unavailable",
    },
  ];
}

/** Maps pinned volume lookup failures to their published recovery guidance. */
function lookupProblem(failure: unknown): string {
  if (failure instanceof AnalysisError) {
    switch (failure.code) {
      case "STALE_VERSION":
        return "This result is no longer current. Open the latest result to explain its volume usage.";
      case "NOT_FOUND":
        return "This scan result is no longer available. Run the analysis again to explain its volume usage.";
      case "INVALID_ARGUMENT":
        if (failure.reasonKey === "space.target_out_of_scope")
          return "The selected target is outside this scan. Choose a folder or volume included in the scan.";
    }
  }
  return "The volume explanation is unavailable.";
}

/** Reads start/end volume observations and difference factors from this result. */
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
  const [root, setRoot] = useState("0");
  const [pending, setPending] = useState(false);
  const [report, setReport] = useState<VolumeGapResponse>();
  const [problem, setProblem] = useState<string>();
  const [wholeVolume, setWholeVolume] = useState(false);
  const sequence = useRef(0);

  async function load(target: VolumeGapRequest["target"]) {
    const requestSeq = ++sequence.current;
    setPending(true);
    setProblem(undefined);
    setReport(undefined);
    setWholeVolume(target.volumeId != null);
    try {
      const response = await client.call("space.volume_gap", {
        resultVersion,
        requestSeq,
        target,
      });
      if (sequence.current !== requestSeq) return;
      if (
        response.resultVersion !== resultVersion ||
        response.requestSeq !== requestSeq
      ) {
        setProblem("The response does not match this scan result.");
        return;
      }
      setReport(response);
    } catch (failure) {
      if (sequence.current !== requestSeq) return;
      setProblem(lookupProblem(failure));
    } finally {
      if (sequence.current === requestSeq) setPending(false);
    }
  }

  function clear() {
    sequence.current += 1;
    setPending(false);
    setReport(undefined);
    setProblem(undefined);
  }

  if (roots.length === 0) return null;
  // The admitted result keeps root items in the reviewed canonical order.
  const options = roots.map((path, index) => ({
    value: String(index),
    label: path.display,
  }));

  return (
    <section aria-label="Volume explanation">
      {roots.length > 1 && (
        <SelectField
          label="Scanned folder"
          value={root}
          options={options}
          onChange={(selected) => {
            clear();
            setRoot(selected);
          }}
        />
      )}
      <Action
        isDisabled={pending}
        onPress={() => {
          if (report) clear();
          else void load({ root: { resultVersion, itemNo: Number(root) } });
        }}
      >
        {report ? "Hide volume explanation" : "Explain volume usage"}
      </Action>
      {pending && <Progress label="Reading volume observations" />}
      {problem && <Notice tone="error">{problem}</Notice>}
      {report && (
        <>
          <Readout
            label="Volume observations"
            rows={[
              ...observationRows("Start", report.start, display),
              ...observationRows("End", report.end, display),
            ]}
          />
          <Readout
            label="Volume difference"
            rows={[
              {
                label: "Compared scope",
                value: wholeVolume
                  ? "All scanned folders on this volume"
                  : "Selected scanned folder",
              },
              {
                label: "Scoped allocated size",
                value: `${displaySizeWithBytes(report.allocated.size, display)} · ${displayMeasurement(report.allocated.size, report.allocated.completeness)}`,
              },
              {
                label: "Scoped unique physical size",
                value: `${displaySizeWithBytes(report.uniquePhysical.size, display)} · ${displayMeasurement(report.uniquePhysical.size, report.uniquePhysical.completeness)}`,
              },
              ...report.factors.map(({ key, value }) => ({
                label: factorLabels[key] ?? key,
                value: amount(value, display),
              })),
              {
                label: "Unexplained difference",
                value: amount(report.remainder, display),
              },
              {
                label: "OS system categories",
                value:
                  report.osCategories.state === "unavailable"
                    ? "Unavailable"
                    : report.osCategories.rows.length === 0
                      ? "None reported"
                      : report.osCategories.rows
                          .map(
                            ({ key, value }) =>
                              `${key} · ${amount(value, display)}`,
                          )
                          .join(", "),
              },
            ]}
          />
          {!wholeVolume && roots.length > 1 && report.volumeId && (
            <Action onPress={() => void load({ volumeId: report.volumeId })}>
              Compare all scanned folders on this volume
            </Action>
          )}
        </>
      )}
    </section>
  );
}
