import { useEffect, useRef, useState } from "react";
import type {
  AnalysisClient,
  AnalysisJobStatus,
  JobState,
  ScanStart,
  SpaceTotal,
} from "../ipc";

/** Determines whether a status may still transition. */
export function isRunning(state: JobState): boolean {
  return !["done", "partial", "failed", "cancelled"].includes(state);
}

/** Tracks a single scan and reads totals only from the reported result version. */
export function useScanResult(
  client: AnalysisClient,
  receipt: ScanStart | null,
) {
  const [status, setStatus] = useState<AnalysisJobStatus>();
  const [total, setTotal] = useState<{ jobRef: string; value: SpaceTotal }>();
  const [error, setError] = useState<{ jobRef: string; message: string }>();
  const sequence = useRef(0);

  useEffect(() => {
    if (!receipt) return;
    let current = true;
    let complete = false;
    let inFlight = false;
    const requestSeq = ++sequence.current;
    const refresh = async () => {
      if (complete || inFlight) return;
      inFlight = true;
      try {
        const next = await client.call("job.get", {
          jobRef: receipt.jobRef,
        });
        if (!current || next.jobRef !== receipt.jobRef) return;
        setStatus(next);
        setError(undefined);
        if (isRunning(next.state)) return;
        complete = true;
        if (!next.resultVersion) return;
        const result = await client.call("space.total", {
          resultVersion: next.resultVersion,
          target: { kind: "scope_root" },
          requestSeq,
        });
        if (
          current &&
          result.requestSeq === requestSeq &&
          result.resultVersion === next.resultVersion
        ) {
          setTotal({ jobRef: receipt.jobRef, value: result });
        }
      } catch (failure) {
        if (current)
          setError({
            jobRef: receipt.jobRef,
            message:
              failure instanceof Error
                ? failure.message
                : "Analysis could not be read",
          });
      } finally {
        inFlight = false;
      }
    };
    const unsubscribe = client.subscribe((event) => {
      if (
        (event.jobRef === receipt.jobRef && event.topic.startsWith("job.")) ||
        event.topic === "job.resync_required"
      ) {
        void refresh();
      }
    });
    const timer = window.setInterval(() => void refresh(), 500);
    void refresh();
    return () => {
      current = false;
      window.clearInterval(timer);
      unsubscribe();
    };
  }, [client, receipt]);

  return {
    status: status?.jobRef === receipt?.jobRef ? status : undefined,
    total: total?.jobRef === receipt?.jobRef ? total?.value : undefined,
    error: error?.jobRef === receipt?.jobRef ? error?.message : undefined,
  };
}
