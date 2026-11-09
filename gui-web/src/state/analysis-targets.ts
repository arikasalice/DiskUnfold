import { useCallback, useEffect, useRef, useState } from "react";
import type { AnalysisClient, TargetList, TargetStatusChanged } from "../ipc";

/** Maintains the target list and accepts only newer probes for its active job. */
export function useTargets(client: AnalysisClient) {
  const [list, setList] = useState<TargetList>();
  const [probes, setProbes] = useState<Record<string, TargetStatusChanged>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const active = useRef<{
    request: number;
    job: string | null;
    sequences: Map<string, number>;
  }>({
    request: 0,
    job: null,
    sequences: new Map(),
  });

  const refresh = useCallback(
    async (requested: boolean) => {
      const request = ++active.current.request;
      active.current.job = null;
      setError(undefined);
      setLoading(true);
      try {
        const response = await client.call("target.list", {
          refresh: requested,
        });
        if (active.current.request !== request) return;
        active.current.job = response.probeJobRef;
        active.current.sequences.clear();
        setProbes({});
        setList(response);
      } catch (error) {
        if (active.current.request === request) {
          setError(
            error instanceof Error ? error.message : "Target listing failed",
          );
        }
      } finally {
        if (active.current.request === request) setLoading(false);
      }
    },
    [client],
  );

  useEffect(() => {
    const state = active.current;
    const unsubscribe = client.subscribe((event) => {
      if (event.topic === "job.resync_required") {
        void refresh(true);
        return;
      }
      if (
        event.topic !== "target.status_changed" ||
        !state.job ||
        event.jobRef !== state.job
      )
        return;
      const status = event.body as TargetStatusChanged;
      const key = status.target.raw;
      if ((state.sequences.get(key) ?? 0) >= event.seq) return;
      state.sequences.set(key, event.seq);
      setProbes((previous) => ({ ...previous, [key]: status }));
    });
    void refresh(false);
    return () => {
      state.request += 1;
      state.job = null;
      unsubscribe();
    };
  }, [client, refresh]);

  return { list, probes, loading, error, refresh };
}
