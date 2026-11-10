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
  displaySize,
  evidenceLabels,
  formatBytes,
} from "./space-format";

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

function amount(value: VolumeGapAmount): string {
  if (value.state === "unknown") return "Unavailable";
  return [formatBytes(value.bytes), evidenceLabels[value.evidence]]
    .filter(Boolean)
    .join(" · ");
}

function observationRows(label: string, observation: VolumeGapObservation) {
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
      value: formatBytes(observation.totalBytes),
    },
    {
      label: `${label} volume used`,
      value: formatBytes(observation.usedBytes),
    },
    {
      label: `${label} volume free`,
      value: formatBytes(observation.freeBytes),
    },
    {
      label: `${label} evidence`,
      value: evidenceLabels[observation.evidence] ?? "Unavailable",
    },
  ];
}

/** Reads start/end volume observations and difference factors from this result. */
export function VolumeExplanation({
  client,
  resultVersion,
  roots,
}: {
  client: AnalysisClient;
  resultVersion: string;
  roots: readonly OsPath[];
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
      setProblem(
        failure instanceof AnalysisError && failure.code === "STALE_VERSION"
          ? "This result is no longer current. Open the latest result to explain its volume usage."
          : "The volume explanation is unavailable.",
      );
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
              ...observationRows("Start", report.start),
              ...observationRows("End", report.end),
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
                value: `${displaySize(report.allocated.size)} · ${displayMeasurement(report.allocated.size, report.allocated.completeness)}`,
              },
              {
                label: "Scoped unique physical size",
                value: `${displaySize(report.uniquePhysical.size)} · ${displayMeasurement(report.uniquePhysical.size, report.uniquePhysical.completeness)}`,
              },
              ...report.factors.map(({ key, value }) => ({
                label: factorLabels[key] ?? key,
                value: amount(value),
              })),
              {
                label: "Unexplained difference",
                value: amount(report.remainder),
              },
              {
                label: "OS system categories",
                value:
                  report.osCategories.state === "unavailable"
                    ? "Unavailable"
                    : report.osCategories.rows.length === 0
                      ? "None reported"
                      : report.osCategories.rows
                          .map(({ key, value }) => `${key} · ${amount(value)}`)
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
