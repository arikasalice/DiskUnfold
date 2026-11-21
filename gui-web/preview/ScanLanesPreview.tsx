import { useRef } from "react";
import { ScanOutputHeader } from "../src/features/analysis/ScanOutputHeader";
import { ScanResourceContext } from "../src/features/analysis/scan-resource-context";
import type { AnalysisClient, AnalysisJobProgress } from "../src/ipc";
import type { useScanResult } from "../src/state/analysis-scan";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import {
  displayInteger,
  RegionalFormatContext,
} from "../src/i18n/regional-format";
import type { RegionalFormat } from "../src/i18n/regional-format";
import { englishText, koreanText } from "./result-summary-fixture";
import "../src/theme/analysis-results.css";
import {
  oneScanLaneStatus,
  scanLanePriorityArea,
  scanLaneRoots,
  scanLaneStatus,
} from "./scan-lanes-fixture";
import { CollectionConditionsPreview } from "./CollectionConditionsPreview";
import type { CollectionPreviewScene } from "./CollectionConditionsPreview";

const regional: RegionalFormat = {
  decimalSeparator: ".",
  groupingSeparator: ",",
  groupingSize: 3,
  datePattern: "yyyy.MM.dd HH:mm:ss",
  dateFormat: "lang_default",
  groupNumbers: true,
  timeZonePolicy: "utc",
  localTimeZoneStatus: "identified",
};
const progress: AnalysisJobProgress = {
  sequence: 27,
  completed: 427,
  totalKnown: false,
  unit: "items",
  waitReason: "none",
  observedAt: "2026-11-20T09:00:00.000Z",
};

/** The real progress header with a fixed scan event; no engine calls are made. */
export function ScanLanesPreview() {
  const params = new URLSearchParams(window.location.search);
  const scene = params.get("scene") ?? "main";
  const locale = params.get("locale") === "en" ? "en" : "ko";
  const localeText = locale === "en" ? englishText : koreanText;
  const laneText: LocaleText = (key, fallback, values) =>
    localeText(
      key,
      fallback,
      Object.fromEntries(
        Object.entries(values ?? {}).map(([name, value]) => [
          name,
          typeof value === "bigint" ? displayInteger(value, regional) : value,
        ]),
      ),
    );
  const versionSummary = useRef<HTMLDivElement>(null);
  if (scene.startsWith("scope-"))
    return (
      <CollectionConditionsPreview scene={scene as CollectionPreviewScene} />
    );
  const oneLane = scene === "one-lane";
  const resourceScene =
    scene === "progress" || scene === "progress-limit-error";
  const errorScene = scene === "progress-limit-error";
  const scan = {
    status: { jobRef: "preview-scan", state: "running" },
    progress,
    laneStatus: oneLane
      ? oneScanLaneStatus
      : errorScene
        ? {
            ...scanLaneStatus,
            lanes: scanLaneStatus.lanes.map((lane) => ({
              ...lane,
              permit: 16,
            })),
          }
        : scanLaneStatus,
  } as ReturnType<typeof useScanResult>;
  const client = {
    call: (async (name: string) => {
      if (name !== "scan.set_resource") throw new Error(`Unavailable ${name}`);
      return {
        outcome: "accepted",
        lanes: scan.laneStatus!.lanes.map((lane) => ({
          laneIndex: lane.laneIndex,
          effectiveLimit: lane.limit,
        })),
        appliesFrom: "next_assignment",
      };
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  } as AnalysisClient;
  const content = (
    <div className="du-scan-lanes-preview">
      <ScanOutputHeader
        scan={scan}
        total={undefined}
        versionSummary={versionSummary}
        started
        running
        holding={false}
        awaitingTotals={false}
        actions={<span />}
        roots={scanLaneRoots}
        priorityArea={scanLanePriorityArea}
      />
    </div>
  );
  return (
    <LocaleTextContext.Provider value={laneText}>
      <RegionalFormatContext.Provider value={regional}>
        {resourceScene ? (
          <ScanResourceContext.Provider
            value={{
              client,
              jobRef: "preview-scan",
              collection: { resourceMode: "auto" },
              active: true,
            }}
          >
            {content}
          </ScanResourceContext.Provider>
        ) : (
          content
        )}
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}
