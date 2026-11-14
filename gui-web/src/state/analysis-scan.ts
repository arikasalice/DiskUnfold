import { useEffect, useRef, useState } from "react";
import { AnalysisError, resultReadProblem } from "../ipc";
import { watchJobBaseline } from "./job-baseline";
import type {
  AnalysisClient,
  AnalysisJobStatus,
  EventEnvelope,
  JobState,
  ScanControl,
  ScanProblem,
  ScanStart,
  SpaceTotal,
} from "../ipc";

export type { ScanControl } from "../ipc";

/** Determines whether a status may still transition. */
export function isRunning(state: JobState): boolean {
  return !["done", "partial", "failed", "cancelled"].includes(state);
}

function canControl(
  command: ScanControl,
  state: JobState | undefined,
): boolean {
  switch (command) {
    case "pause":
      return state === "running";
    case "resume":
      return state === "pause_requested" || state === "paused";
    case "cancel":
      return state !== "cancel_requested";
  }
}

function controlObserved(command: ScanControl, state: JobState): boolean {
  if (state === "cancel_requested") return true;
  switch (command) {
    case "pause":
      return state === "pause_requested" || state === "paused";
    case "resume":
      return state === "running";
    case "cancel":
      return false;
  }
}

function controlProblem(
  command: ScanControl,
  outcome: string | undefined,
): ScanProblem {
  if (outcome === "already_terminal") return { kind: "already_ended" };
  return outcome === undefined || outcome === "unsupported"
    ? { kind: "control_unavailable", command }
    : { kind: "control_rejected", command };
}

/** Whether the document is hidden, so polling and frame work can pause. */
function documentHidden(): boolean {
  return typeof document !== "undefined" && document.hidden;
}

type ControlView = {
  jobRef: string;
  command?: ScanControl;
  problem?: ScanProblem;
};

function controlsForJob(
  jobRef: string | undefined,
  status: AnalysisJobStatus | undefined,
  unavailable: boolean,
  control: ControlView | undefined,
) {
  const running =
    jobRef !== undefined &&
    !unavailable &&
    (!status || isRunning(status.state));
  const current = running && control?.jobRef === jobRef ? control : undefined;
  return {
    running,
    control: current?.command,
    controlError: current?.problem,
  };
}

function isNewLineageEvent(
  event: EventEnvelope,
  lineage: string | undefined,
  sequence: number,
): boolean {
  const body = event.body;
  return (
    event.topic === "results.version_changed" &&
    lineage !== undefined &&
    body !== null &&
    typeof body === "object" &&
    "lineageId" in body &&
    body.lineageId === lineage &&
    "sequence" in body &&
    typeof body.sequence === "number" &&
    Number.isSafeInteger(body.sequence) &&
    body.sequence > sequence
  );
}

/** Opens a scan's first available result and keeps it pinned in the view. */
export function useScanResult(
  client: AnalysisClient,
  receipt: ScanStart | null,
) {
  const [status, setStatus] = useState<AnalysisJobStatus>();
  const [total, setTotal] = useState<{ jobRef: string; value: SpaceTotal }>();
  const [latest, setLatest] = useState<{ jobRef: string; version: string }>();
  const [switchingJob, setSwitchingJob] = useState<string>();
  const [error, setError] = useState<{
    jobRef: string;
    problem: ScanProblem;
    unavailable: boolean;
  }>();
  const [control, setControl] = useState<ControlView>();
  const issueControl = useRef<((command: ScanControl) => void) | undefined>(
    undefined,
  );
  const sequence = useRef(0);
  const showLatest = useRef<() => void>(undefined);
  const jobRef = receipt?.jobRef;
  const receiptLineage = receipt?.lineageId;

  useEffect(() => {
    if (!jobRef) return;
    const scanJob = jobRef;
    let alive = true;
    let terminal = false;
    let terminalPending = false;
    let inFlight = false;
    let queued = false;
    let queuedResult = false;
    let scheduledResult = false;
    let displayLatest = false;
    let switching = false;
    let lineage = receiptLineage;
    let jobState: JobState | undefined;
    let pending:
      | {
          command: ScanControl;
          after: number;
          acknowledged?: number;
        }
      | undefined;
    let jobSequence = -1;
    let resultSequence = -1;
    let resultAtSequence: string | null | undefined;
    let displayed: string | undefined;
    let rejected: string | undefined;
    let staleFailure: AnalysisError | undefined;
    let statusReadFailed = false;
    let statusFrame: number | undefined;
    let hiddenWork = false;

    function clearControl() {
      pending = undefined;
      setControl((current) =>
        current?.jobRef === scanJob ? undefined : current,
      );
    }

    async function requestControl(command: ScanControl) {
      if (!alive || terminal || !canControl(command, jobState)) return;
      if (pending && (command !== "cancel" || pending.command === "cancel"))
        return;

      const owner: NonNullable<typeof pending> = {
        command,
        after: jobSequence,
      };
      pending = owner;
      setControl({ jobRef: scanJob, command });
      try {
        const response = await client.call(`job.${command}`, {
          jobRef: scanJob,
        });
        if (!alive || terminal || pending !== owner) return;
        if (response.sequence < jobSequence) {
          clearControl();
          return;
        }
        if (response.outcome === "accepted") {
          owner.acknowledged = response.sequence;
        } else {
          clearControl();
          setControl({
            jobRef: scanJob,
            problem: controlProblem(command, response.outcome),
          });
        }
        void refresh();
      } catch {
        if (!alive || terminal || pending !== owner) return;
        clearControl();
        setControl({
          jobRef: scanJob,
          problem: controlProblem(command, undefined),
        });
      }
    }

    function receiveControlStatus(state: JobState) {
      if (
        pending &&
        (terminal ||
          (jobSequence > pending.after &&
            jobSequence >= (pending.acknowledged ?? 0) &&
            controlObserved(pending.command, state)))
      )
        clearControl();
    }

    async function readTotals(version: string) {
      const requestSeq = ++sequence.current;
      try {
        const result = await client.call("space.total", {
          resultVersion: version,
          target: { kind: "scope_root" },
          requestSeq,
        });
        if (!alive) return;
        if (
          result.requestSeq !== requestSeq ||
          result.resultVersion !== version
        ) {
          rejected = version;
          displayLatest = false;
          recordProblem({ kind: "totals_mismatch" }, false);
          return "totals" as const;
        }
        displayed = version;
        displayLatest = false;
        rejected = undefined;
        staleFailure = undefined;
        setTotal({ jobRef: scanJob, value: result });
        if (baseline.ready()) setError(undefined);
      } catch (failure) {
        if (!alive) return;
        rejected = version;
        if (
          failure instanceof AnalysisError &&
          (failure.code === "STALE_VERSION" || failure.code === "NOT_FOUND")
        ) {
          // A partial result can be superseded before its first read is admitted.
          staleFailure = failure;
          queued = true;
          queuedResult = true;
          return "resync" as const;
        }
        displayLatest = false;
        recordError(failure, false);
      }
      return "totals" as const;
    }

    function recordError(failure: unknown, unavailable: boolean) {
      recordProblem(
        { kind: "read", problem: resultReadProblem(failure) },
        unavailable,
      );
    }

    function recordProblem(problem: ScanProblem, unavailable: boolean) {
      setError({ jobRef: scanJob, unavailable, problem });
    }

    function matchesLineage(result: AnalysisJobStatus["scanResult"]) {
      return !result || !lineage || result.lineageId === lineage;
    }

    async function readStatus() {
      let next: AnalysisJobStatus;
      try {
        next = await client.call("job.get", { jobRef: scanJob });
      } catch (failure) {
        if (!alive) return;
        if (!baseline.ready()) return;
        statusReadFailed = true;
        const unavailable =
          failure instanceof AnalysisError && failure.code === "NOT_FOUND";
        if (unavailable) {
          terminal = true;
          if (pending) clearControl();
        }
        recordError(failure, unavailable);
        return;
      }
      if (!alive || next.jobRef !== scanJob) return;
      const result = next.scanResult;
      if (!matchesLineage(result)) return;
      if (!baseline.ready() || next.sequence < baseline.sequence()) {
        if (result) return reconcileResult(next);
        return;
      }
      if (
        next.sequence < jobSequence ||
        (jobState !== undefined &&
          !isRunning(jobState) &&
          isRunning(next.state))
      )
        return;
      if (statusReadFailed) {
        statusReadFailed = false;
        setError(undefined);
      }
      jobSequence = next.sequence;
      jobState = next.state;
      terminal = !isRunning(next.state);
      terminalPending = terminal && next.terminal === undefined;
      receiveControlStatus(next.state);
      setStatus(next);
      return reconcileResult(next);
    }

    async function reconcileResult(next: AnalysisJobStatus) {
      const result = next.scanResult;
      if (result) {
        // A lineage sequence names one immutable publication, including a withdrawal.
        if (
          result.sequence < resultSequence ||
          (result.sequence === resultSequence &&
            result.resultVersion !== resultAtSequence)
        )
          return;
        lineage = result.lineageId;
        resultSequence = result.sequence;
        resultAtSequence = result.resultVersion;
      }
      const version = result
        ? result.resultVersion
        : terminal
          ? next.resultVersion
          : undefined;
      if (!version || version === displayed) displayLatest = false;
      setLatest((previous) =>
        version === undefined || version === null
          ? undefined
          : previous?.jobRef === scanJob && previous.version === version
            ? previous
            : { jobRef: scanJob, version },
      );
      if (!version && next.state === "failed") {
        displayed = undefined;
        displayLatest = false;
        setTotal(undefined);
        if (baseline.ready()) setError(undefined);
        return;
      }
      if (
        version &&
        version !== displayed &&
        version !== rejected &&
        (!displayed || displayLatest)
      ) {
        return readTotals(version);
      } else if (terminal && version === rejected && staleFailure) {
        displayLatest = false;
        recordError(staleFailure, false);
      }
    }

    function finishSwitch() {
      if (!switching) return;
      switching = false;
      displayLatest = false;
      setSwitchingJob((current) => (current === scanJob ? undefined : current));
    }

    async function refresh(resultOnly = false) {
      if (!alive || (!baseline.ready() && !resultOnly)) return;
      if (inFlight) {
        queued = true;
        queuedResult ||= resultOnly;
        return;
      }
      inFlight = true;
      const startedForSwitch = switching;
      let outcome: Awaited<ReturnType<typeof readStatus>> = undefined;
      try {
        outcome = await readStatus();
      } finally {
        inFlight = false;
        // A status read already in flight can fulfill the selection by reading
        // totals. Otherwise the queued read still owns the new selection.
        if (
          alive &&
          outcome !== "resync" &&
          (startedForSwitch || outcome === "totals")
        )
          finishSwitch();
        if (queued && alive) {
          queued = false;
          const retryResult = queuedResult;
          queuedResult = false;
          void refresh(retryResult);
        }
      }
    }

    issueControl.current = (command) => void requestControl(command);
    showLatest.current = () => {
      if (switching) return;
      switching = true;
      setSwitchingJob(scanJob);
      displayLatest = true;
      rejected = undefined;
      staleFailure = undefined;
      void refresh(true);
    };
    // Notifications invalidate status. Read it once for all events in this frame.
    function scheduleStatusRead(resultOnly = false) {
      scheduledResult ||= resultOnly;
      // A hidden window renders no frames; catch up when it becomes visible.
      if (documentHidden()) {
        hiddenWork = true;
        return;
      }
      if (statusFrame !== undefined) return;
      statusFrame = window.requestAnimationFrame(() => {
        statusFrame = undefined;
        const readResult = scheduledResult;
        scheduledResult = false;
        void refresh(readResult);
      });
    }

    function pollDue() {
      return baseline.ready() && (!terminal || terminalPending);
    }

    function visibilityChanged() {
      if (documentHidden()) {
        if (statusFrame === undefined) return;
        window.cancelAnimationFrame(statusFrame);
        statusFrame = undefined;
        hiddenWork = true;
        return;
      }
      const due = hiddenWork || pollDue();
      hiddenWork = false;
      if (!due || statusFrame !== undefined) return;
      const readResult = scheduledResult;
      scheduledResult = false;
      void refresh(readResult);
    }

    const baseline = watchJobBaseline(
      client,
      scanJob,
      scheduleStatusRead,
      (failure) => {
        if (alive) {
          statusReadFailed = true;
          recordError(failure, false);
        }
      },
    );
    const unsubscribe = client.subscribe((event) => {
      if (
        event.topic === "job.resync_required" ||
        isNewLineageEvent(event, lineage, resultSequence)
      ) {
        scheduleStatusRead(true);
      }
    });
    const timer = window.setInterval(() => {
      if (
        !documentHidden() &&
        !inFlight &&
        statusFrame === undefined &&
        pollDue()
      )
        void refresh();
    }, 500);
    document.addEventListener("visibilitychange", visibilityChanged);
    void refresh();
    return () => {
      finishSwitch();
      alive = false;
      showLatest.current = undefined;
      issueControl.current = undefined;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", visibilityChanged);
      if (statusFrame !== undefined) window.cancelAnimationFrame(statusFrame);
      baseline.dispose();
      unsubscribe();
    };
  }, [client, jobRef, receiptLineage]);

  const currentError = error?.jobRef === jobRef ? error : undefined;
  const currentTotal = total?.jobRef === jobRef ? total?.value : undefined;
  const currentStatus = status?.jobRef === jobRef ? status : undefined;
  const unavailable = currentError?.unavailable ?? false;
  return {
    status: currentStatus,
    ...controlsForJob(jobRef, currentStatus, unavailable, control),
    requestControl: (command: ScanControl) => issueControl.current?.(command),
    total: currentTotal,
    hasNewResult:
      currentTotal !== undefined &&
      latest?.jobRef === jobRef &&
      latest?.version !== currentTotal.resultVersion,
    showLatest: () => showLatest.current?.(),
    switchingResult: switchingJob !== undefined && switchingJob === jobRef,
    error: currentError?.problem,
    unavailable,
  };
}
