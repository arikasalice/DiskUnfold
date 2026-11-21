import { useLayoutEffect, useRef, useState } from "react";
import type { ReactNode, Ref } from "react";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import {
  Action,
  Heading,
  LiveStatus,
  Progress,
  StatusBadge,
  Text,
} from "../../components";
import type {
  AnalysisJobProgress,
  JobState,
  OsPath,
  SpaceTotal,
} from "../../ipc";
import type { ScanControl, useScanResult } from "../../state/analysis-scan";
import type { ScanPriorityArea } from "../../state/scan-priority";
import type { ScanIssueLaneSelection } from "./scan-issue-filter";
import { ScanLaneProgress } from "./ScanLaneProgress";

/** How the shown values relate to a scan that is still in progress. */
type LiveResult = "updating" | "so_far";

/** How long an ended scan's final result may take before the header says it is being read. */
export const FINAL_RESULT_NOTICE_MS = 300;

function liveResult(
  running: boolean,
  state: JobState | undefined,
): LiveResult | undefined {
  if (!running) return undefined;
  return state === undefined || state === "queued" || state === "running"
    ? "updating"
    : "so_far";
}

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

/**
 * The final result of an ended scan while the summary still shows an earlier
 * one, or nothing when no read is under way: the scan still runs, the user
 * holds the shown values with a selection, or the read failed (the existing
 * error and "Show latest result" then apply).
 */
function awaitedFinalResult(
  scan: ReturnType<typeof useScanResult>,
  total: SpaceTotal | undefined,
  running: boolean,
  holding: boolean,
): string | undefined {
  if (running || holding || !total || scan.error || scan.hasNewResult)
    return undefined;
  return scan.currentVersion === total.resultVersion
    ? undefined
    : scan.currentVersion;
}

/**
 * Whether `awaited` has been awaited for `delay` without a break. A result
 * that arrives sooner is never reported as overdue, so nothing flickers.
 */
function useOverdue(awaited: string | undefined, delay: number): boolean {
  const [overdue, setOverdue] = useState<string>();
  useLayoutEffect(() => {
    setOverdue(undefined);
    if (awaited === undefined) return;
    const timer = window.setTimeout(() => setOverdue(awaited), delay);
    return () => window.clearTimeout(timer);
  }, [awaited, delay]);
  return awaited !== undefined && overdue === awaited;
}

function progressLabel(
  state: JobState | undefined,
  cancelling: boolean,
  text: LocaleText,
) {
  if (cancelling)
    return text("ui.analysis.waiting_cancel", "Waiting for cancellation");
  if (state === "queued")
    return text(
      "ui.analysis.waiting_scan_start",
      "Waiting for the scan to start",
    );
  return text("ui.analysis.scanning_folder", "Scanning folder");
}

function progressPaused(state: JobState | undefined, cancelling: boolean) {
  return !cancelling && state === "paused";
}

function LaneProgressLine({
  scan,
  roots,
  priorityArea,
  onViewUnreadableLocations,
}: {
  scan: ReturnType<typeof useScanResult>;
  roots: readonly OsPath[];
  priorityArea?: ScanPriorityArea | undefined;
  onViewUnreadableLocations?:
    ((selection: ScanIssueLaneSelection) => void) | undefined;
}) {
  if (!scan.status?.jobRef || !scan.laneStatus) return null;
  return (
    <ScanLaneProgress
      jobRef={scan.status.jobRef}
      status={scan.laneStatus}
      roots={roots}
      priorityArea={priorityArea}
      onViewUnreadableLocations={onViewUnreadableLocations}
    />
  );
}

/** Heads the scan output with its status, result version, controls and progress. */
export function ScanOutputHeader({
  scan,
  total,
  versionSummary,
  started,
  running,
  holding,
  awaitingTotals,
  actions,
  roots,
  priorityArea,
  onViewUnreadableLocations,
}: {
  scan: ReturnType<typeof useScanResult>;
  roots: readonly OsPath[];
  /** The totals the summary shows, which may be held during a text selection. */
  total: SpaceTotal | undefined;
  versionSummary: Ref<HTMLDivElement>;
  started: boolean;
  running: boolean;
  /** Whether a text selection holds the shown totals back from newer ones. */
  holding: boolean;
  awaitingTotals: boolean;
  actions: ReactNode;
  /** The folder name retained by this GUI for its priority request. */
  priorityArea?: ScanPriorityArea | undefined;
  onViewUnreadableLocations?:
    ((selection: ScanIssueLaneSelection) => void) | undefined;
}) {
  const text = useLocaleText();
  const cancelling =
    scan.control === "cancel" || scan.status?.state === "cancel_requested";
  const readingFinal = useOverdue(
    awaitedFinalResult(scan, total, running, holding),
    FINAL_RESULT_NOTICE_MS,
  );
  return (
    <header className="du-result__header">
      <div className="du-result__title">
        <Heading level={3}>
          {total
            ? text("ui.analysis.result_heading", "Analysis result")
            : text("ui.analysis.progress_heading", "Scan progress")}
        </Heading>
        {started && (
          <ScanStatus
            state={scan.unavailable ? "unavailable" : scan.status?.state}
            pending={running ? scan.control : undefined}
          />
        )}
      </div>
      <div className="du-result__toolbar">
        <ResultVersionSummary
          ref={versionSummary}
          total={total}
          live={liveResult(running, scan.status?.state)}
          hasNewResult={scan.hasNewResult}
          showLatest={scan.showLatest}
          switching={scan.switchingResult}
          trailing={total?.resultVersion !== scan.total?.resultVersion}
          finished={scan.status?.state === "done"}
          readingFinal={readingFinal}
        />
        {actions}
      </div>
      {running && (
        <div className="du-scan-progress">
          <Progress
            label={progressLabel(scan.status?.state, cancelling, text)}
            paused={progressPaused(scan.status?.state, cancelling)}
            {...determinateProgress(scan.progress)}
          />
          {/* Not a live region: a running count would be read on every frame. */}
          {scan.progress && (
            <Text kind="muted">{progressText(scan.progress, text)}</Text>
          )}
          <LaneProgressLine
            scan={scan}
            roots={roots}
            priorityArea={priorityArea}
            onViewUnreadableLocations={onViewUnreadableLocations}
          />
        </div>
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

/** A bar claims a share of the work only when the total is confirmed. */
function determinateProgress(progress: AnalysisJobProgress | undefined) {
  if (!progress?.totalKnown || progress.total === undefined) return {};
  return { value: progress.completed, maxValue: progress.total };
}

/**
 * With a confirmed total, the share is rounded down so the card never reads
 * 100% before the work is done, and the rest is what job.progress still owes.
 * Integer arithmetic keeps byte counts above 2^53 / 100 exact.
 */
function progressShare(completed: number, total: number) {
  const whole = BigInt(Math.trunc(total));
  const done = BigInt(Math.trunc(Math.min(completed, total)));
  return {
    percent: whole === 0n ? 100n : (done * 100n) / whole,
    remaining: whole - done,
  };
}

function progressText(progress: AnalysisJobProgress, text: LocaleText) {
  const completed = BigInt(progress.completed);
  const total = progress.totalKnown ? progress.total : undefined;
  if (total !== undefined) {
    const { percent, remaining } = progressShare(progress.completed, total);
    return progress.unit === "bytes"
      ? text(
          "ui.analysis.progress_bytes_remaining",
          "{percent}% · {size} left",
          {
            percent,
            size: remaining,
          },
        )
      : text(
          "ui.analysis.progress_items_remaining",
          "{percent}% · {count} items left",
          { percent, count: remaining },
        );
  }
  if (progress.unit === "bytes")
    return text(
      "ui.analysis.progress_bytes",
      "{size} checked · Total size not yet known",
      { size: completed },
    );
  return text(
    "ui.analysis.progress_items",
    "{count} items checked · Counting all items",
    { count: completed },
  );
}

/**
 * The scan state as a badge inside a status region, which announces each
 * change once. The badge is not a focus stop: it cannot be pressed.
 */
function ScanStatus({
  state = "queued",
  pending,
}: {
  state: JobState | "unavailable" | undefined;
  pending: ScanControl | undefined;
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
    <LiveStatus>
      <StatusBadge tone={state === "done" ? "good" : "info"}>
        {statusText}
      </StatusBadge>
    </LiveStatus>
  );
}

function ResultVersionSummary({
  ref,
  total,
  live,
  hasNewResult,
  showLatest,
  switching,
  trailing,
  finished,
  readingFinal,
}: {
  ref: Ref<HTMLDivElement>;
  total: SpaceTotal | undefined;
  /** Set while the shown result belongs to the scan still in progress. */
  live: LiveResult | undefined;
  hasNewResult: boolean;
  showLatest: () => void;
  switching: boolean;
  /**
   * Set while `total` is not yet the totals the scan read last: the summary
   * shows them together with an open part, or holds them during a selection.
   */
  trailing: boolean;
  finished: boolean;
  /**
   * Set once an ended scan's final result has been awaited for a moment. The
   * values stay as received, and change together with the result version
   * when it arrives.
   */
  readingFinal: boolean;
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
  // A chosen result is being read until the summary shows it, which can be
  // after the scan has read its totals.
  const reading = switching || (waitingToSwitch && trailing);
  // The summary follows its running scan by itself; an explicit choice is
  // offered only once the scan has stopped and its newest result could not
  // be shown automatically.
  const offerLatest = hasNewResult && live === undefined;

  useLayoutEffect(() => {
    if (!waitingToSwitch || offerLatest || reading) return;
    if (!version || version === selection.version) {
      setSelection(undefined);
      return;
    }
    // Keep the action mounted through this render so we can check its focus
    // before removing it. Do not move focus if the user already moved on.
    if (button.current && button.current === document.activeElement)
      summary.current?.focus({ preventScroll: true });
    setSelection({ version, shown: true });
  }, [offerLatest, reading, selection, version, waitingToSwitch]);

  const shownLatest = selection?.shown && selection.version === version;
  return (
    <div
      ref={ref}
      className="du-result-version"
      data-result={total ? "" : undefined}
    >
      <div>
        <LiveStatus ref={summary} focusTarget={total !== undefined}>
          {total && (
            <Text kind="muted">
              {text("ui.analysis.result_version", "Result version: {status}", {
                status: resultVersionLabel(total.completeness, text),
              })}
            </Text>
          )}
          {offerLatest && (
            <Text>{newerResultMessage(finished, total, text)}</Text>
          )}
          {!offerLatest && shownLatest && (
            <Text>
              {text(
                "ui.analysis.latest_visible",
                "Now showing the latest result.",
              )}
            </Text>
          )}
        </LiveStatus>
        {/* Outside the status region: the scan state badge already announces pause and resume. */}
        {total && live && <LiveResultLine live={live} />}
        {readingFinal && <FinalResultLine />}
      </div>
      {version && (offerLatest || waitingToSwitch) && (
        <Action
          ref={button}
          isPending={reading}
          onPress={() => {
            setSelection({ version, shown: false });
            showLatest();
          }}
        >
          {reading
            ? text("ui.analysis.reading_latest", "Reading latest result…")
            : text("ui.analysis.show_latest", "Show latest result")}
        </Action>
      )}
    </div>
  );
}

/** Says how the shown values relate to the scan still in progress. */
function LiveResultLine({ live }: { live: LiveResult }) {
  const text = useLocaleText();
  return (
    <Text kind="muted">
      {live === "updating"
        ? text(
            "ui.analysis.values_update_live",
            "Values update as the scan runs",
          )
        : text("ui.analysis.result_so_far", "Result so far")}
    </Text>
  );
}

/** Takes the place of the live line while an ended scan's final result is read. */
function FinalResultLine() {
  const text = useLocaleText();
  return (
    <Text kind="muted">
      {text("ui.analysis.reading_final", "Reading the final result…")}
    </Text>
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
