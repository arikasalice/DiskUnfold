import { useCallback, useEffect, useRef, useState } from "react";
import type { AnalysisClient, TargetList, TargetProbeEvent } from "../ipc";

type Probe = { job: string; body: TargetProbeEvent };

type ProbeWindow = {
  request: number;
  job: string | null;
  sequence: number;
  targets: Set<string>;
  early: Probe[] | null;
};

/** Accepts only increasing observations for a target in the active list. */
function accept(window: ProbeWindow, { job, body }: Probe): boolean {
  if (
    job !== window.job ||
    body.sequence <= window.sequence ||
    !window.targets.has(body.target.raw)
  ) {
    return false;
  }
  window.sequence = body.sequence;
  return true;
}

/** Maintains the target list and accepts only newer probes for its active job. */
export function useTargets(client: AnalysisClient) {
  const [list, setList] = useState<TargetList>();
  const [probes, setProbes] = useState<Record<string, TargetProbeEvent>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const active = useRef<ProbeWindow>({
    request: 0,
    job: null,
    sequence: 0,
    targets: new Set(),
    early: null,
  });

  const refresh = useCallback(
    async (requested: boolean) => {
      const state = active.current;
      const request = ++state.request;
      state.job = null;
      state.early = [];
      setError(undefined);
      setLoading(true);
      try {
        const response = await client.call("target.list", {
          refresh: requested,
        });
        if (state.request !== request) return;
        state.job = response.probeJobRef;
        state.sequence = 0;
        state.targets = new Set(response.targets.map(({ path }) => path.raw));
        // An OS probe may finish before the IPC reply identifying its job.
        const early = state.early ?? [];
        state.early = null;
        setProbes(
          Object.fromEntries(
            early
              .filter((probe) => accept(state, probe))
              .map(({ body }) => [body.target.raw, body]),
          ),
        );
        setList(response);
      } catch (error) {
        if (state.request === request) {
          state.early = null;
          setError(
            error instanceof Error ? error.message : "Target listing failed",
          );
        }
      } finally {
        if (state.request === request) setLoading(false);
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
      if (event.topic !== "target.status_changed" || !event.jobRef) return;
      const body = event.body as TargetProbeEvent | undefined;
      if (
        !body?.target?.raw ||
        !Number.isSafeInteger(body.sequence) ||
        body.sequence < 1
      ) {
        return;
      }
      const probe = { job: event.jobRef, body };
      if (state.early) {
        if (state.early.length < 1_024) state.early.push(probe);
        return;
      }
      if (accept(state, probe)) {
        setProbes((previous) => ({ ...previous, [body.target.raw]: body }));
      }
    });
    void refresh(false);
    return () => {
      state.request += 1;
      state.job = null;
      state.early = null;
      unsubscribe();
    };
  }, [client, refresh]);

  return { list, probes, loading, error, refresh };
}
