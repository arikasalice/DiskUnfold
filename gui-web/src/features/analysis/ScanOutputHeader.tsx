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
import type { AnalysisJobProgress, JobState, SpaceTotal } from "../../ipc";
import type { ScanControl, useScanResult } from "../../state/analysis-scan";

/** How the shown values relate to a scan that is still in progress. */
type LiveResult = "updating" | "so_far";

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

/** Heads the scan output with its status, result version, controls and progress. */
export function ScanOutputHeader({
  scan,
  total,
  versionSummary,
  started,
  running,
  awaitingTotals,
  statusFocus,
  actions,
}: {
  scan: ReturnType<typeof useScanResult>;
  /** The totals the summary shows, which may be held during a text selection. */
  total: SpaceTotal | undefined;
  versionSummary: Ref<HTMLDivElement>;
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
          {total
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
          ref={versionSummary}
          total={total}
          live={liveResult(running, scan.status?.state)}
          hasNewResult={scan.hasNewResult}
          showLatest={scan.showLatest}
          switching={scan.switchingResult}
          trailing={total?.resultVersion !== scan.total?.resultVersion}
          finished={scan.status?.state === "done"}
        />
        {actions}
      </div>
      {running && (
        <div className="du-scan-progress">
          <Progress
            label={
              cancelling
                ? text("ui.analysis.waiting_cancel", "Waiting for cancellation")
                : text("ui.analysis.scanning_folder", "Scanning folder")
            }
            paused={!cancelling && scan.status?.state === "paused"}
            {...determinateProgress(scan.progress)}
          />
          {/* Not a live region: a running count would be read on every frame. */}
          {scan.progress && (
            <Text kind="muted">{progressText(scan.progress, text)}</Text>
          )}
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

function ResultVersionSummary({
  ref,
  total,
  live,
  hasNewResult,
  showLatest,
  switching,
  trailing,
  finished,
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
