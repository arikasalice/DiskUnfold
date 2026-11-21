import { useRef, useState } from "react";
import { ScanOutputHeader } from "../src/features/analysis/ScanOutputHeader";
import { ScanIssues } from "../src/features/analysis/ScanIssues";
import type {
  ScanIssueLaneRequest,
  ScanIssueLaneSelection,
} from "../src/features/analysis/scan-issue-filter";
import { ScanResourceContext } from "../src/features/analysis/scan-resource-context";
import type {
  AnalysisClient,
  AnalysisJobProgress,
  ScanFailuresRequest,
} from "../src/ipc";
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
  const failureScene = scene.startsWith("failure-");
  const listScene = failureScene && scene !== "failure-lanes";
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
  const requestId = useRef(0);
  const [laneRequest, setLaneRequest] = useState<ScanIssueLaneRequest>();
  if (scene.startsWith("scope-"))
    return (
      <CollectionConditionsPreview scene={scene as CollectionPreviewScene} />
    );
  const oneLane = scene === "one-lane";
  const resourceScene =
    scene === "progress" || scene === "progress-limit-error";
  const errorScene = scene === "progress-limit-error";
  const baseScan = {
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
  const scan = failureScene
    ? {
        ...baseScan,
        laneStatus: {
          ...baseScan.laneStatus!,
          lanes: baseScan.laneStatus!.lanes.map((lane, index) => ({
            ...lane,
            failures: index === 0 ? "2" : "0",
          })),
        },
      }
    : baseScan;
  const client = {
    call: (async (name: string, body: ScanFailuresRequest) => {
      if (name === "scan.set_resource")
        return {
          outcome: "accepted",
          lanes: scan.laneStatus!.lanes.map((lane) => ({
            laneIndex: lane.laneIndex,
            effectiveLimit: lane.limit,
          })),
          appliesFrom: "next_assignment",
        };
      if (name === "scan.failures")
        return {
          resultVersion: body.resultVersion,
          totalCount: 2,
          rows: [
            {
              kind: "unread_range",
              stage: "enumerate",
              pathRef: { rootIndex: 0, relative: [] },
              reasonKey: "scan.path_denied",
              displayPath: "C:\\Users\\민서\\문서\\잠김",
            },
            {
              kind: "collection_failed",
              stage: "open_directory",
              pathRef: { rootIndex: 1, relative: [] },
              reasonKey: "scan.unclassified",
              displayPath: "C:\\Users\\민서\\사진\\대기",
            },
          ],
        };
      throw new Error(`Unavailable ${name}`);
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  } as AnalysisClient;
  const onViewUnreadableLocations = (selection: ScanIssueLaneSelection) => {
    setLaneRequest({ ...selection, requestId: ++requestId.current });
  };
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
        onViewUnreadableLocations={onViewUnreadableLocations}
      />
      {listScene && (
        <ScanIssues
          client={client}
          resultVersion="preview-failures"
          onReopenLatest={() => {}}
          running
          laneRequest={laneRequest}
        />
      )}
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
