import { useEffect, useRef, useState } from "react";
import { AnalysisError } from "../ipc";
import type {
  AnalysisClient,
  AnalysisJobStatus,
  EventEnvelope,
  JobState,
  ScanStart,
  SpaceTotal,
} from "../ipc";

/** Determines whether a status may still transition. */
export function isRunning(state: JobState): boolean {
  return !["done", "partial", "failed", "cancelled"].includes(state);
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
  const [error, setError] = useState<{
    jobRef: string;
    message: string;
    unavailable: boolean;
  }>();
  const sequence = useRef(0);
  const showLatest = useRef<() => void>(undefined);
  const jobRef = receipt?.jobRef;
  const receiptLineage = receipt?.lineageId;

  useEffect(() => {
    if (!jobRef) return;
    const scanJob = jobRef;
    let alive = true;
    let terminal = false;
    let inFlight = false;
    let queued = false;
    let displayLatest = false;
    let lineage = receiptLineage;
    let jobSequence = -1;
    let resultSequence = -1;
    let displayed: string | undefined;
    let rejected: string | undefined;
    let staleFailure: AnalysisError | undefined;
    let statusReadFailed = false;

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
          throw new Error("The space totals do not match this scan's result.");
        }
        displayed = version;
        displayLatest = false;
        rejected = undefined;
        staleFailure = undefined;
        setTotal({ jobRef: scanJob, value: result });
        setError(undefined);
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
          return;
        }
        displayLatest = false;
        recordError(failure, false);
      }
    }

    function recordError(failure: unknown, unavailable: boolean) {
      setError({
        jobRef: scanJob,
        unavailable,
        message:
          failure instanceof Error
            ? failure.message
            : "Analysis could not be read",
      });
    }

    async function readStatus() {
      let next: AnalysisJobStatus;
      try {
        next = await client.call("job.get", { jobRef: scanJob });
      } catch (failure) {
        if (!alive) return;
        statusReadFailed = true;
        const unavailable =
          failure instanceof AnalysisError && failure.code === "NOT_FOUND";
        if (unavailable) terminal = true;
        recordError(failure, unavailable);
        return;
      }
      if (!alive || next.jobRef !== scanJob || next.sequence < jobSequence)
        return;
      const result = next.scanResult;
      if (result && lineage && result.lineageId !== lineage) return;
      if (statusReadFailed) {
        statusReadFailed = false;
        setError(undefined);
      }
      jobSequence = next.sequence;
      terminal = !isRunning(next.state);
      setStatus(next);
      await reconcileResult(next);
    }

    async function reconcileResult(next: AnalysisJobStatus) {
      const result = next.scanResult;
      if (result) {
        if (result.sequence < resultSequence) return;
        lineage = result.lineageId;
        resultSequence = result.sequence;
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
        setError(undefined);
        return;
      }
      if (
        version &&
        version !== displayed &&
        version !== rejected &&
        (!displayed || displayLatest)
      ) {
        await readTotals(version);
      } else if (terminal && version === rejected && staleFailure) {
        displayLatest = false;
        recordError(staleFailure, false);
      }
    }

    async function refresh() {
      if (!alive) return;
      if (inFlight) {
        queued = true;
        return;
      }
      inFlight = true;
      try {
        await readStatus();
      } finally {
        inFlight = false;
        if (queued && alive) {
          queued = false;
          void refresh();
        }
      }
    }

    showLatest.current = () => {
      displayLatest = true;
      rejected = undefined;
      staleFailure = undefined;
      void refresh();
    };
    const unsubscribe = client.subscribe((event) => {
      if (
        (event.jobRef === scanJob && event.topic.startsWith("job.")) ||
        event.topic === "job.resync_required" ||
        isNewLineageEvent(event, lineage, resultSequence)
      ) {
        void refresh();
      }
    });
    const timer = window.setInterval(() => {
      if (!terminal) void refresh();
    }, 500);
    void refresh();
    return () => {
      alive = false;
      showLatest.current = undefined;
      window.clearInterval(timer);
      unsubscribe();
    };
  }, [client, jobRef, receiptLineage]);

  const currentError = error?.jobRef === jobRef ? error : undefined;
  const currentTotal = total?.jobRef === jobRef ? total?.value : undefined;
  return {
    status: status?.jobRef === jobRef ? status : undefined,
    total: currentTotal,
    hasNewResult:
      currentTotal !== undefined &&
      latest?.jobRef === jobRef &&
      latest?.version !== currentTotal.resultVersion,
    showLatest: () => showLatest.current?.(),
    error: currentError?.message,
    unavailable: currentError?.unavailable ?? false,
  };
}
