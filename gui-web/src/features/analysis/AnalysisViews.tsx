import { useLayoutEffect, useRef, useState } from "react";
import type { ReactNode, Ref } from "react";
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
import { SIZE_UNIT_RULES } from "../../ipc";
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

const resultVersionLabels = {
  complete: "Final",
  partial: "Interim",
  missing: "Final with scan gaps",
  cancelled: "Cancelled",
} satisfies Record<SpaceTotal["completeness"], string>;

function Coverage({ total }: { total: SpaceTotal }) {
  const counters = [
    { label: "Missing items", count: total.missingItems },
    { label: "Inaccessible ranges", count: total.inaccessibleRanges },
    { label: "Untraversed links", count: total.untraversedLinks },
  ];
  return (
    <Readout
      label="Coverage limits"
      rows={counters.map(({ label, count }) => ({
        label,
        value: count === null ? "Not recorded" : BigInt(count).toLocaleString(),
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
  scan: ReturnType<typeof useScanResult>;
  started: boolean;
  running: boolean;
  problem: string | undefined;
  statusFocus: Ref<HTMLDivElement>;
  roots: readonly OsPath[];
  actions: ReactNode;
}) {
  const pathsLabel = running ? "Scanning paths" : "Selected paths";
  const error =
    problem ?? scan.controlError ?? scan.error ?? scan.status?.reasonKey;
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
        aria-label="Result details"
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
          />
        ) : running ? (
          <Text kind="muted">
            {scan.status?.state === "paused"
              ? "The scan is paused. Resume to continue collecting file metadata."
              : "Collecting file metadata. Totals appear when a result is ready."}
          </Text>
        ) : !awaitingTotals && !error ? (
          <Text kind="muted">{emptyResultMessage(scan.status?.state)}</Text>
        ) : null}
      </section>
    </>
  );
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
  const cancelling =
    scan.control === "cancel" || scan.status?.state === "cancel_requested";
  return (
    <header className="du-result__header">
      <div className="du-result__title">
        <Heading level={3}>
          {scan.total ? "Analysis result" : "Scan progress"}
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
          label={cancelling ? "Waiting for cancellation" : "Scanning folder"}
        />
      )}
      {awaitingTotals && (
        <div className="du-result__loading">
          <LiveStatus>
            <Text>Reading space totals…</Text>
          </LiveStatus>
          <Progress label="Reading space totals" />
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
  const labels: Record<JobState | "unavailable", string> = {
    queued: "Waiting to scan",
    running: "Scanning",
    pause_requested: "Pause requested",
    paused: "Scan paused",
    cancel_requested: "Cancellation requested",
    done: "Scan finished",
    partial: "Scan ended with gaps",
    failed: "Scan failed",
    cancelled: "Scan cancelled",
    unavailable: "Scan status unavailable",
  };
  const statusText =
    pending === "cancel" || state === "cancel_requested"
      ? labels.cancel_requested
      : pending === "resume"
        ? "Resume requested"
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

function emptyResultMessage(state: JobState | undefined): string {
  switch (state) {
    case "cancelled":
      return "The scan was cancelled before a result was available.";
    case "failed":
      return "The scan failed before a result was available.";
    case "partial":
      return "The scan ended with gaps and no result is available.";
    case "done":
      return "The scan finished without an available result.";
    default:
      return "No result is available yet.";
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
            Result version: {resultVersionLabels[total.completeness]}
          </Text>
        )}
        {hasNewResult && <Text>{newerResultMessage(finished, total)}</Text>}
        {!hasNewResult && shownLatest && (
          <Text>Now showing the latest result.</Text>
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
          {switching ? "Reading latest result…" : "Show latest result"}
        </Action>
      )}
    </div>
  );
}

function newerResultMessage(
  finished: boolean,
  total: SpaceTotal | undefined,
): string {
  if (!finished)
    return "Newer result available. Showing the current result until you switch.";
  return total?.completeness === "partial"
    ? "You’re still viewing an interim result."
    : "You’re still viewing an earlier result.";
}

/** Displays the selected result without replacing it on later publications. */
function PinnedResult({
  client,
  total,
  roots,
  preferences,
  displayChoice,
  onDisplayChoice,
}: {
  client: AnalysisClient;
  total: SpaceTotal;
  roots: readonly OsPath[];
  preferences: AnalysisDisplayPreferences;
  displayChoice: SizeDisplay | undefined;
  onDisplayChoice: (choice: SizeDisplay) => void;
}) {
  const [basisChoice, setBasis] = useState<SizeBasisFilter>();
  const display = displayChoice ?? preferences.display;
  const basis = basisChoice ?? preferences.basis;
  const lead = total.sizes.find(({ size }) => size.basis === basis);
  const unit =
    display.unit === "auto"
      ? "Automatic"
      : (SIZE_UNIT_RULES[display.system].symbols[display.unit] ??
        "Unavailable");
  const basisLabel = sizeLabels[basis];
  return (
    <div className="du-results">
      <Readout
        label="Scanned paths"
        rows={[
          {
            label: "Scanned paths",
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
          {lead ? displaySize(lead.size, display) : "Unavailable"}
        </Metric>
      </div>
      <Coverage total={total} />
      <DetailDisclosure title="Measurement details">
        <Readout
          label="Measurement details"
          rows={total.sizes.map(({ size, completeness, measurementLimits }) => {
            const value = displaySize(size, display);
            const raw = displayRawSize(size);
            return {
              label: sizeLabels[size.basis],
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
          Some display preferences could not be restored. Using the available
          settings and defaults.
        </Notice>
      )}
      <DetailDisclosure title="Display options">
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
        key={`issues:${total.resultVersion}`}
        client={client}
        resultVersion={total.resultVersion}
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
    <section aria-label="Folder details">
      <Action ref={trigger} onPress={() => setOpen(true)}>
        View folder details
      </Action>
      {open && (
        <InspectionDialog
          title="Folder details"
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
