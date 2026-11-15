import { useRef } from "react";
import type { ReactNode, Ref } from "react";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { Notice, Readout, Text } from "../../components";
import { scanJobReasonText, scanProblemText } from "../../ipc";
import type { AnalysisClient, JobState, OsPath } from "../../ipc";
import type { useScanResult } from "../../state/analysis-scan";
import { PinnedResult, RootPaths } from "./ResultSummary";
import { ScanOutputHeader } from "./ScanOutputHeader";
import { useHeldValue, useSelectionHold } from "./use-selection-hold";
import type { SizeBasisFilter, SizeDisplay } from "./space-format";
import type { AnalysisDisplayPreferences } from "./use-display-preferences";
import "../../theme/analysis-results.css";

/**
 * Shows the scan's newest totals, with basis and coverage kept separate.
 *
 * Values change in place as the scan publishes. While the user selects text
 * in the summary, the whole summary keeps the result it showed when the
 * selection began; the hook keeps reading, so releasing the selection shows
 * the newest totals at once. No older result is held by the engine.
 */
export function ScanOutput({
  client,
  preferences,
  displayChoice,
  onDisplayChoice,
  onSizeBasisChange,
  scan,
  started,
  running,
  problem,
  statusFocus,
  roots,
  actions,
}: {
  client: AnalysisClient;
  preferences: AnalysisDisplayPreferences;
  displayChoice: SizeDisplay | undefined;
  onDisplayChoice: (choice: SizeDisplay) => void;
  onSizeBasisChange?:
    ((resultVersion: string, basis: SizeBasisFilter) => void) | undefined;
  scan: ReturnType<typeof useScanResult>;
  started: boolean;
  running: boolean;
  problem: string | undefined;
  statusFocus: Ref<HTMLDivElement>;
  roots: readonly OsPath[];
  actions: ReactNode;
}) {
  const text = useLocaleText();
  const body = useRef<HTMLElement>(null);
  const versionSummary = useRef<HTMLDivElement>(null);
  const holding = useSelectionHold([body, versionSummary]);
  const total = useHeldValue(scan.total, holding);
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
        awaitingTotals={awaitingTotals}
        statusFocus={statusFocus}
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
            roots={roots}
            preferences={preferences}
            displayChoice={displayChoice}
            onDisplayChoice={onDisplayChoice}
            onSizeBasisChange={onSizeBasisChange}
            onReopenLatest={scan.showLatest}
          />
        ) : running ? (
          <Text kind="muted">
            {scan.status?.state === "paused"
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
          <Text kind="muted">
            {emptyResultMessage(scan.status?.state, text)}
          </Text>
        ) : null}
      </section>
    </>
  );
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
