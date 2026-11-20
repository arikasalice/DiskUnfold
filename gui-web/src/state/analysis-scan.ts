import { useEffect, useRef, useState } from "react";
import { AnalysisError, resultReadProblem } from "../ipc";
import { watchJobBaseline } from "./job-baseline";
import { createReflectCadence } from "./result-cadence";
import {
  canControl,
  controlObserved,
  controlProblem,
  controlsForJob,
  documentHidden,
  isNewLineageEvent,
  isRunning,
  jobValue,
  runningValue,
} from "./scan-job-rules";
import type { ControlView } from "./scan-job-rules";
import type {
  AnalysisClient,
  AnalysisJobProgress,
  AnalysisJobStatus,
  JobState,
  ScanControl,
  ScanLaneStatus,
  ScanProblem,
  ScanStart,
  SpaceTotal,
} from "../ipc";

export type { ScanControl } from "../ipc";
export { isRunning } from "./scan-job-rules";

/** Follows a scan's result publications and its latest progress values. */
export function useScanResult(
  client: AnalysisClient,
  receipt: ScanStart | null,
) {
  const [status, setStatus] = useState<AnalysisJobStatus>();
  const [progress, setProgress] = useState<{
    jobRef: string;
    value: AnalysisJobProgress;
  }>();
  const [laneStatus, setLaneStatus] = useState<{
    jobRef: string;
    value: ScanLaneStatus;
  }>();
  const [total, setTotal] = useState<{ jobRef: string; value: SpaceTotal }>();
  const [latest, setLatest] = useState<{ jobRef: string; version: string }>();
  const [stalled, setStalled] = useState<{ jobRef: string; version: string }>();
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
    let frame: number | undefined;
    let statusDue = false;
    let queuedProgress: AnalysisJobProgress | undefined;
    let hiddenWork = false;
    const cadence = createReflectCadence();

    function reject(version: string) {
      rejected = version;
      setStalled({ jobRef: scanJob, version });
    }

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
      cadence.mark();
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
          reject(version);
          displayLatest = false;
          recordProblem({ kind: "totals_mismatch" }, false);
          return "totals" as const;
        }
        displayed = version;
        displayLatest = false;
        rejected = undefined;
        staleFailure = undefined;
        setStalled(undefined);
        setTotal({ jobRef: scanJob, value: result });
        if (baseline.ready()) setError(undefined);
      } catch (failure) {
        if (!alive) return;
        reject(version);
        if (
          failure instanceof AnalysisError &&
          (failure.code === "STALE_VERSION" || failure.code === "NOT_FOUND")
        ) {
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
      if (terminal) queuedProgress = undefined;
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
      if (version && version !== displayed && version !== rejected) {
        return readWhenDue(version);
      } else if (terminal && version === rejected && staleFailure) {
        displayLatest = false;
        recordError(staleFailure, false);
      }
    }

    function readWhenDue(version: string) {
      if (displayed && !displayLatest && cadence.remaining() > 0) {
        cadence.defer(() => {
          if (alive) scheduleStatusRead(true);
        });
        return;
      }
      return readTotals(version);
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
    // Notifications invalidate status. Read it once for all events in this
    // frame, and draw only the latest progress value received in that frame.
    function scheduleFrame() {
      if (frame !== undefined) return;
      frame = window.requestAnimationFrame(() => {
        frame = undefined;
        drawProgress();
        if (!statusDue) return;
        statusDue = false;
        const readResult = scheduledResult;
        scheduledResult = false;
        void refresh(readResult);
      });
    }

    function scheduleStatusRead(resultOnly = false) {
      scheduledResult ||= resultOnly;
      if (documentHidden()) {
        hiddenWork = true;
        return;
      }
      statusDue = true;
      scheduleFrame();
    }

    function receiveProgress(value: AnalysisJobProgress) {
      if (!alive || terminal) return;
      queuedProgress = value;
      if (!documentHidden()) scheduleFrame();
    }

    function drawProgress() {
      const value = queuedProgress;
      queuedProgress = undefined;
      if (!value || !alive || terminal) return;
      setProgress({ jobRef: scanJob, value });
    }

    function pollDue() {
      return baseline.ready() && (!terminal || terminalPending);
    }

    function visibilityChanged() {
      if (documentHidden()) {
        if (frame === undefined) return;
        window.cancelAnimationFrame(frame);
        frame = undefined;
        if (statusDue) hiddenWork = true;
        statusDue = false;
        return;
      }
      drawProgress();
      const due = hiddenWork || pollDue();
      hiddenWork = false;
      if (!due || frame !== undefined) return;
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
      receiveProgress,
      (value) => setLaneStatus({ jobRef: scanJob, value }),
      () =>
        setLaneStatus((current) =>
          current?.jobRef === scanJob ? undefined : current,
        ),
    );
    const unsubscribe = client.subscribe((event) => {
      if (event.topic === "job.resync_required") queuedProgress = undefined;
      if (
        event.topic === "job.resync_required" ||
        isNewLineageEvent(event, lineage, resultSequence)
      ) {
        scheduleStatusRead(true);
      }
    });
    const timer = window.setInterval(() => {
      if (!documentHidden() && !inFlight && !statusDue && pollDue())
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
      cadence.dispose();
      document.removeEventListener("visibilitychange", visibilityChanged);
      if (frame !== undefined) window.cancelAnimationFrame(frame);
      baseline.dispose();
      unsubscribe();
    };
  }, [client, jobRef, receiptLineage]);

  const currentError = error?.jobRef === jobRef ? error : undefined;
  const currentTotal = total?.jobRef === jobRef ? total?.value : undefined;
  const currentStatus = status?.jobRef === jobRef ? status : undefined;
  const unavailable = currentError?.unavailable ?? false;
  const latestVersion = jobValue(latest, jobRef)?.version;
  return {
    status: currentStatus,
    /** The latest ordered job.progress value while the scan is running. */
    progress: runningValue(progress, jobRef, currentStatus),
    laneStatus: runningValue(laneStatus, jobRef, currentStatus),
    /** The scan's newest published result version, which may not be displayed yet. */
    currentVersion: latestVersion,
    ...controlsForJob(jobRef, currentStatus, unavailable, control),
    requestControl: (command: ScanControl) => issueControl.current?.(command),
    total: currentTotal,
    /** A newer result whose automatic read failed; the user may select it. */
    hasNewResult:
      currentTotal !== undefined &&
      latestVersion !== undefined &&
      latestVersion !== currentTotal.resultVersion &&
      jobValue(stalled, jobRef)?.version === latestVersion,
    showLatest: () => showLatest.current?.(),
    switchingResult: switchingJob !== undefined && switchingJob === jobRef,
    error: currentError?.problem,
    unavailable,
  };
}
