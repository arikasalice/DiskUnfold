import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode, Ref } from "react";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import { displayInteger, useRegionalFormat } from "../../i18n/regional-format";
import {
  Action,
  DetailDisclosure,
  Heading,
  InspectionDialog,
  LiveStatus,
  Metric,
  Notice,
  Progress,
  Readout,
  StatusBadge,
  Text,
} from "../../components";
import { SIZE_UNIT_RULES, scanJobReasonText, scanProblemText } from "../../ipc";
import type { AnalysisClient, JobState, OsPath, SpaceTotal } from "../../ipc";
import type { ScanControl, useScanResult } from "../../state/analysis-scan";
import { ScanIssues } from "./ScanIssues";
import { FileInspection } from "./FileInspection";
import { SizeDisplayControls } from "./SizeDisplayControls";
import { VolumeExplanation } from "./VolumeExplanation";
import {
  displayMeasurement,
  displayMeasurementLimit,
  displaySize,
  displayRawSize,
  sizeLabels,
} from "./space-format";
import type { SizeBasisFilter, SizeDisplay } from "./space-format";
import type { AnalysisDisplayPreferences } from "./use-display-preferences";
import "../../theme/analysis-results.css";

function resultVersionLabel(
  state: SpaceTotal["completeness"],
  text: LocaleText,
) {
  switch (state) {
    case "complete":
      return text("ui.analysis.version_final", "Final");
    case "partial":
      return text("ui.analysis.version_interim", "Interim");
    case "missing":
      return text("ui.analysis.version_gaps", "Final with scan gaps");
    case "cancelled":
      return text("ui.analysis.version_cancelled", "Cancelled");
  }
}

function Coverage({ total }: { total: SpaceTotal }) {
  const regional = useRegionalFormat();
  const text = useLocaleText();
  const counters = [
    {
      label: text("ui.analysis.missing_items", "Missing items"),
      count: total.missingItems,
    },
    {
      label: text("ui.analysis.inaccessible_ranges", "Inaccessible ranges"),
      count: total.inaccessibleRanges,
    },
    {
      label: text("ui.analysis.untraversed_links", "Untraversed links"),
      count: total.untraversedLinks,
    },
  ];
  return (
    <Readout
      label={text("ui.analysis.coverage_limits", "Coverage limits")}
      rows={counters.map(({ label, count }) => ({
        label,
        value:
          count === null
            ? text("ui.analysis.not_recorded", "Not recorded")
            : displayInteger(BigInt(count), regional),
      }))}
    />
  );
}

/** Shows only totals from the scan's pinned result, with basis and coverage kept separate. */
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
  const pathsLabel = running
    ? text("ui.analysis.scanning_paths", "Scanning paths")
    : text("ui.analysis.selected_paths", "Selected paths");
  const error = problem ?? scanErrorText(scan, text);
  const awaitingTotals =
    !running &&
    Boolean(scan.status?.resultVersion) &&
    !scan.total &&
    !scan.error;
  return (
    <>
      <ScanOutputHeader
        scan={scan}
        started={started}
        running={running}
        awaitingTotals={awaitingTotals}
        statusFocus={statusFocus}
        actions={actions}
      />
      <section
        className="du-result__body"
        aria-label={text("ui.analysis.result_details", "Result details")}
        tabIndex={0}
      >
        {error && <Notice tone="error">{error}</Notice>}
        {started && !scan.total && (
          <Readout
            label={pathsLabel}
            rows={[
              {
                label: pathsLabel,
                value: roots.map((root) => root.display).join(", "),
              },
            ]}
          />
        )}
        {scan.total ? (
          <PinnedResult
            client={client}
            total={scan.total}
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

function ScanOutputHeader({
  scan,
  started,
  running,
  awaitingTotals,
  statusFocus,
  actions,
}: {
  scan: ReturnType<typeof useScanResult>;
  started: boolean;
  running: boolean;
  awaitingTotals: boolean;
  statusFocus: Ref<HTMLDivElement>;
  actions: ReactNode;
}) {
  const text = useLocaleText();
  const cancelling =
    scan.control === "cancel" || scan.status?.state === "cancel_requested";
  return (
    <header className="du-result__header">
      <div className="du-result__title">
        <Heading level={3}>
          {scan.total
            ? text("ui.analysis.result_heading", "Analysis result")
            : text("ui.analysis.progress_heading", "Scan progress")}
        </Heading>
        {started && (
          <ScanStatus
            state={scan.unavailable ? "unavailable" : scan.status?.state}
            pending={running ? scan.control : undefined}
            focusTarget={statusFocus}
          />
        )}
      </div>
      <div className="du-result__toolbar">
        <ResultVersionSummary
          total={scan.total}
          hasNewResult={scan.hasNewResult}
          showLatest={scan.showLatest}
          switching={scan.switchingResult}
          finished={scan.status?.state === "done"}
        />
        {actions}
      </div>
      {running && (cancelling || scan.status?.state !== "paused") && (
        <Progress
          label={
            cancelling
              ? text("ui.analysis.waiting_cancel", "Waiting for cancellation")
              : text("ui.analysis.scanning_folder", "Scanning folder")
          }
        />
      )}
      {awaitingTotals && (
        <div className="du-result__loading">
          <LiveStatus>
            <Text>
              {text(
                "ui.analysis.reading_totals_pending",
                "Reading space totals…",
              )}
            </Text>
          </LiveStatus>
          <Progress
            label={text("ui.analysis.reading_totals", "Reading space totals")}
          />
        </div>
      )}
    </header>
  );
}

function ScanStatus({
  state = "queued",
  pending,
  focusTarget,
}: {
  state: JobState | "unavailable" | undefined;
  pending: ScanControl | undefined;
  focusTarget: Ref<HTMLDivElement>;
}) {
  const text = useLocaleText();
  const labels: Record<JobState | "unavailable", string> = {
    queued: text("ui.analysis.waiting_scan", "Waiting to scan"),
    running: text("ui.analysis.scanning", "Scanning"),
    pause_requested: text("ui.analysis.pause_requested", "Pause requested"),
    paused: text("ui.analysis.scan_paused", "Scan paused"),
    cancel_requested: text(
      "ui.analysis.cancel_requested",
      "Cancellation requested",
    ),
    done: text("ui.analysis.scan_finished", "Scan finished"),
    partial: text("ui.analysis.scan_partial", "Scan ended with gaps"),
    failed: text("ui.analysis.scan_failed", "Scan failed"),
    cancelled: text("ui.analysis.scan_cancelled", "Scan cancelled"),
    unavailable: text(
      "ui.analysis.status_unavailable",
      "Scan status unavailable",
    ),
  };
  const statusText =
    pending === "cancel" || state === "cancel_requested"
      ? labels.cancel_requested
      : pending === "resume"
        ? text("ui.analysis.resume_requested", "Resume requested")
        : pending === "pause" || state === "pause_requested"
          ? labels.pause_requested
          : labels[state];
  return (
    <LiveStatus ref={focusTarget} focusTarget>
      <StatusBadge tone={state === "done" ? "good" : "info"}>
        {statusText}
      </StatusBadge>
    </LiveStatus>
  );
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

function ResultVersionSummary({
  total,
  hasNewResult,
  showLatest,
  switching,
  finished,
}: {
  total: SpaceTotal | undefined;
  hasNewResult: boolean;
  showLatest: () => void;
  switching: boolean;
  finished: boolean;
}) {
  const text = useLocaleText();
  const button = useRef<HTMLButtonElement>(null);
  const summary = useRef<HTMLDivElement>(null);
  const [selection, setSelection] = useState<{
    version: string;
    shown: boolean;
  }>();
  const version = total?.resultVersion;
  const waitingToSwitch = selection?.shown === false;

  useLayoutEffect(() => {
    if (!waitingToSwitch || hasNewResult || switching) return;
    if (!version || version === selection.version) {
      setSelection(undefined);
      return;
    }
    // Keep the action mounted through this render so we can check its focus
    // before removing it. Do not move focus if the user already moved on.
    if (button.current && button.current === document.activeElement)
      summary.current?.focus({ preventScroll: true });
    setSelection({ version, shown: true });
  }, [hasNewResult, selection, switching, version, waitingToSwitch]);

  const shownLatest = selection?.shown && selection.version === version;
  return (
    <div className="du-result-version" data-result={total ? "" : undefined}>
      <LiveStatus ref={summary} focusTarget={total !== undefined}>
        {total && (
          <Text kind="muted">
            {text("ui.analysis.result_version", "Result version: {status}", {
              status: resultVersionLabel(total.completeness, text),
            })}
          </Text>
        )}
        {hasNewResult && (
          <Text>{newerResultMessage(finished, total, text)}</Text>
        )}
        {!hasNewResult && shownLatest && (
          <Text>
            {text(
              "ui.analysis.latest_visible",
              "Now showing the latest result.",
            )}
          </Text>
        )}
      </LiveStatus>
      {version && (hasNewResult || waitingToSwitch) && (
        <Action
          ref={button}
          isPending={switching}
          onPress={() => {
            setSelection({ version, shown: false });
            showLatest();
          }}
        >
          {switching
            ? text("ui.analysis.reading_latest", "Reading latest result…")
            : text("ui.analysis.show_latest", "Show latest result")}
        </Action>
      )}
    </div>
  );
}

function newerResultMessage(
  finished: boolean,
  total: SpaceTotal | undefined,
  text: LocaleText,
): string {
  if (!finished)
    return text(
      "ui.analysis.newer_available",
      "Newer result available. Showing the current result until you switch.",
    );
  return total?.completeness === "partial"
    ? text(
        "ui.analysis.still_interim",
        "You’re still viewing an interim result.",
      )
    : text(
        "ui.analysis.still_earlier",
        "You’re still viewing an earlier result.",
      );
}

/** Displays the selected result without replacing it on later publications. */
function PinnedResult({
  client,
  total,
  roots,
  preferences,
  displayChoice,
  onDisplayChoice,
  onSizeBasisChange,
  onReopenLatest,
}: {
  client: AnalysisClient;
  total: SpaceTotal;
  roots: readonly OsPath[];
  preferences: AnalysisDisplayPreferences;
  displayChoice: SizeDisplay | undefined;
  onDisplayChoice: (choice: SizeDisplay) => void;
  onSizeBasisChange?:
    ((resultVersion: string, basis: SizeBasisFilter) => void) | undefined;
  onReopenLatest: () => void;
}) {
  const [basisChoice, setBasis] = useState<SizeBasisFilter>();
  const regional = useRegionalFormat();
  const text = useLocaleText();
  const choice = displayChoice ?? preferences.display;
  const display = regional ? { ...choice, regional } : choice;
  const basis = basisChoice ?? preferences.basis;
  useEffect(() => {
    onSizeBasisChange?.(total.resultVersion, basis);
  }, [basis, onSizeBasisChange, total.resultVersion]);
  const lead = total.sizes.find(({ size }) => size.basis === basis);
  const unavailable = text("ui.analysis.unavailable", "Unavailable");
  const unit =
    display.unit === "auto"
      ? text("ui.analysis.automatic", "Automatic")
      : (SIZE_UNIT_RULES[display.system].symbols[display.unit] ?? unavailable);
  const basisLabels = {
    logical: text("ui.analysis.logical_size", sizeLabels.logical),
    allocated: text("ui.analysis.allocated_size", sizeLabels.allocated),
    unique_physical: text(
      "ui.analysis.unique_physical_size",
      sizeLabels.unique_physical,
    ),
  };
  const basisLabel = basisLabels[basis];
  const pathsLabel = text("ui.analysis.scanned_paths", "Scanned paths");
  const measurements = text("ui.analysis.measurements", "Measurement details");
  return (
    <div className="du-results">
      <Readout
        label={pathsLabel}
        rows={[
          {
            label: pathsLabel,
            value: roots.map((root) => root.display).join(", "),
          },
        ]}
      />
      <div className="du-metrics">
        <Metric
          label={basisLabel}
          detail={
            lead ? displayMeasurement(lead.size, lead.completeness) : undefined
          }
        >
          {lead ? displaySize(lead.size, display, text) : unavailable}
        </Metric>
      </div>
      <Coverage total={total} />
      <DetailDisclosure title={measurements}>
        <Readout
          label={measurements}
          rows={total.sizes.map(({ size, completeness, measurementLimits }) => {
            const value = displaySize(size, display, text);
            const raw = displayRawSize(size, regional, text);
            return {
              label: basisLabels[size.basis],
              value,
              wideValue: true,
              notes: [
                ...(raw && raw !== value ? [raw] : []),
                displayMeasurement(size, completeness),
                ...measurementLimits.map(displayMeasurementLimit),
              ],
            };
          })}
        />
      </DetailDisclosure>
      <Text kind="muted">
        {basisLabel} · {display.system.toUpperCase()} · {unit}
      </Text>
      {preferences.warning && (
        <Notice>
          {text(
            "ui.analysis.display_restore_warning",
            "Some display preferences could not be restored. Using the available settings and defaults.",
          )}
        </Notice>
      )}
      <DetailDisclosure
        title={text("ui.analysis.display_options", "Display options")}
      >
        <SizeDisplayControls
          basis={basis}
          display={display}
          onBasis={setBasis}
          onDisplay={onDisplayChoice}
        />
      </DetailDisclosure>
      <VolumeExplanation
        key={`volume:${total.resultVersion}`}
        client={client}
        resultVersion={total.resultVersion}
        roots={roots}
        display={display}
      />
      <ScanIssues
        client={client}
        resultVersion={total.resultVersion}
        onReopenLatest={onReopenLatest}
      />
      <ScopeDetails
        key={total.resultVersion}
        client={client}
        resultVersion={total.resultVersion}
        multiRoot={roots.length > 1}
        display={display}
      />
    </div>
  );
}

/** Shows only the observed attributes of the pinned scope root. */
function ScopeDetails({
  client,
  resultVersion,
  multiRoot,
  display,
}: {
  client: AnalysisClient;
  resultVersion: string;
  multiRoot: boolean;
  display: SizeDisplay;
}) {
  const [open, setOpen] = useState(false);
  const text = useLocaleText();
  const title = text("ui.analysis.folder_details", "Folder details");
  const trigger = useRef<HTMLButtonElement>(null);
  const restore = useRef(false);
  useLayoutEffect(() => {
    if (!open && restore.current) {
      trigger.current?.focus();
      restore.current = false;
    }
  }, [open]);

  if (multiRoot) return null;

  return (
    <section aria-label={title}>
      <Action ref={trigger} onPress={() => setOpen(true)}>
        {text("ui.analysis.view_folder_details", "View folder details")}
      </Action>
      {open && (
        <InspectionDialog
          title={title}
          onClose={() => {
            restore.current = true;
            setOpen(false);
          }}
        >
          <FileInspection
            client={client}
            item={{ resultVersion, itemNo: 0 }}
            itemKind="folder"
            display={display}
          />
        </InspectionDialog>
      )}
    </section>
  );
}
