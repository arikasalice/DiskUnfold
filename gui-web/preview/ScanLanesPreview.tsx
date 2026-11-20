import { useRef } from "react";
import { ScanOutputHeader } from "../src/features/analysis/ScanOutputHeader";
import type { AnalysisJobProgress } from "../src/ipc";
import type { useScanResult } from "../src/state/analysis-scan";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import {
  displayInteger,
  RegionalFormatContext,
} from "../src/i18n/regional-format";
import type { RegionalFormat } from "../src/i18n/regional-format";
import { koreanText } from "./result-summary-fixture";
import "../src/theme/analysis-results.css";
import {
  oneScanLaneStatus,
  scanLaneRoots,
  scanLaneStatus,
} from "./scan-lanes-fixture";

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
const laneText: LocaleText = (key, fallback, values) =>
  koreanText(
    key,
    fallback,
    Object.fromEntries(
      Object.entries(values ?? {}).map(([name, value]) => [
        name,
        typeof value === "bigint" ? displayInteger(value, regional) : value,
      ]),
    ),
  );

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
  const oneLane = params.get("scene") === "one-lane";
  const versionSummary = useRef<HTMLDivElement>(null);
  const scan = {
    status: { jobRef: "preview-scan", state: "running" },
    progress,
    laneStatus: oneLane ? oneScanLaneStatus : scanLaneStatus,
  } as ReturnType<typeof useScanResult>;
  return (
    <LocaleTextContext.Provider value={laneText}>
      <RegionalFormatContext.Provider value={regional}>
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
          />
        </div>
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}
