import { useRef } from "react";
import type { ReactNode } from "react";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { Notice, Readout, Text } from "../../components";
import { scanJobReasonText, scanProblemText } from "../../ipc";
import type {
  AnalysisClient,
  JobState,
  OsPath,
  ScopeExclusion,
} from "../../ipc";
import type { useScanResult } from "../../state/analysis-scan";
import { PinnedResult, RootPaths } from "./ResultSummary";
import { ScanOutputHeader } from "./ScanOutputHeader";
import { useSelectionHold } from "./use-selection-hold";
import { useSummaryBatch } from "./summary-batch";
import type { SizeBasisFilter, SizeDisplay, SizeWait } from "./space-format";
import type { AnalysisDisplayPreferences } from "./use-display-preferences";
import "../../theme/analysis-results.css";

/**
 * Shows the scan's newest totals, with basis and coverage kept separate.
 *
 * Values change in place as the scan publishes. While the user selects text
 * in the summary, the whole summary keeps the result it showed when the
 * selection began; the hook keeps reading, so releasing the selection shows
 * the newest totals at once. No older result is held by the engine.
 * An open volume part changes together with the totals (see
 * `useSummaryBatch`), so the summary never shows values from two results. A
 * paused scan publishes nothing further, so the part does not wait for a
 * next result then.
 */
export function ScanOutput({
  client,
  preferences,
  displayChoice,
  onDisplayChoice,
  onSizeBasisChange,
  scan,
  job,
  started,
  running,
  problem,
  roots,
  exclusions,
  actions,
}: {
  client: AnalysisClient;
  preferences: AnalysisDisplayPreferences;
  displayChoice: SizeDisplay | undefined;
  onDisplayChoice: (choice: SizeDisplay) => void;
  onSizeBasisChange?:
    ((resultVersion: string, basis: SizeBasisFilter) => void) | undefined;
  scan: ReturnType<typeof useScanResult>;
  /** The scan whose results are shown; a new scan has another reference. */
  job: string | undefined;
  started: boolean;
  running: boolean;
  problem: string | undefined;
  roots: readonly OsPath[];
  /** The exclusion rules of the scope the scan was started with, if known. */
  exclusions: readonly ScopeExclusion[] | undefined;
  actions: ReactNode;
}) {
  const text = useLocaleText();
  const body = useRef<HTMLElement>(null);
  const versionSummary = useRef<HTMLDivElement>(null);
  const holding = useSelectionHold([body, versionSummary]);
  const state = scan.status?.state;
  const publishing = running && state !== "paused";
  const summary = useSummaryBatch(client, scan.total, holding, publishing);
  const total = summary.total;
  const pathsLabel = running
    ? text("ui.analysis.scanning_paths", "Scanning paths")
    : text("ui.analysis.selected_paths", "Selected paths");
  const error = problem ?? scanErrorText(scan, text);
  const awaitingTotals =
    !running && Boolean(scan.status?.resultVersion) && !total && !scan.error;
  return (
    <>
      <ScanOutputHeader
        scan={scan}
        total={total}
        versionSummary={versionSummary}
        started={started}
        running={running}
        holding={holding}
        awaitingTotals={awaitingTotals}
        actions={actions}
      />
      <section
        ref={body}
        className="du-result__body"
        aria-label={text("ui.analysis.result_details", "Result details")}
        tabIndex={0}
      >
        {error && <Notice tone="error">{error}</Notice>}
        {started && !total && (
          <Readout
            label={pathsLabel}
            rows={[
              {
                label: pathsLabel,
                value: <RootPaths roots={roots} />,
              },
            ]}
          />
        )}
        {total ? (
          <PinnedResult
            client={client}
            total={total}
            latestVersion={scan.currentVersion ?? total.resultVersion}
            volume={summary.volume}
            scan={job}
            wait={sizeWait(running, state)}
            roots={roots}
            exclusions={exclusions}
            preferences={preferences}
            displayChoice={displayChoice}
            onDisplayChoice={onDisplayChoice}
            onSizeBasisChange={onSizeBasisChange}
            onReopenLatest={scan.showLatest}
          />
        ) : running ? (
          <Text kind="muted">
            {state === "paused"
              ? text(
                  "ui.analysis.paused_hint",
                  "The scan is paused. Resume to continue collecting file metadata.",
                )
              : text(
                  "ui.analysis.collecting_hint",
                  "Collecting file metadata. Totals appear when a result is ready.",
                )}
          </Text>
        ) : !awaitingTotals && !error ? (
          <Text kind="muted">{emptyResultMessage(state, text)}</Text>
        ) : null}
      </section>
    </>
  );
}

/**
 * How the shown scan stands for a size it has not supplied yet. A scan being
 * cancelled, like one that ended, supplies nothing further.
 */
function sizeWait(
  running: boolean,
  state: JobState | undefined,
): SizeWait | undefined {
  if (!running || state === "cancel_requested") return undefined;
  return state === "paused" ? "paused" : "running";
}

function scanErrorText(
  scan: ReturnType<typeof useScanResult>,
  text: LocaleText,
): string | undefined {
  const problem = scan.controlError ?? scan.error;
  if (problem) return scanProblemText(problem, text);
  const reason = scan.status?.reasonKey;
  return reason ? scanJobReasonText(reason, text) : undefined;
}

function emptyResultMessage(
  state: JobState | undefined,
  text: LocaleText,
): string {
  switch (state) {
    case "cancelled":
      return text(
        "ui.analysis.empty_cancelled",
        "The scan was cancelled before a result was available.",
      );
    case "failed":
      return text(
        "ui.analysis.empty_failed",
        "The scan failed before a result was available.",
      );
    case "partial":
      return text(
        "ui.analysis.empty_partial",
        "The scan ended with gaps and no result is available.",
      );
    case "done":
      return text(
        "ui.analysis.empty_done",
        "The scan finished without an available result.",
      );
    default:
      return text("ui.analysis.empty_result", "No result is available yet.");
  }
}
