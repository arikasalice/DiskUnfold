import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AnalysisClient,
  EventEnvelope,
  TargetList,
  TargetProbeEvent,
} from "../ipc";

type ProbeUpdate = {
  job: string;
  sequence: number;
  observation?: TargetProbeEvent;
};

type ProbeWindow = {
  request: number;
  job: string | null;
  sequence: number;
  targets: Set<string>;
  finished: boolean;
  early: ProbeUpdate[] | null;
};

/** Reads either a target observation or the probe job's terminal marker. */
function decode(event: EventEnvelope): ProbeUpdate | undefined {
  if (!event.jobRef) return;
  const body = event.body as Partial<TargetProbeEvent> | null;
  const sequence = body?.sequence;
  if (
    typeof sequence !== "number" ||
    !Number.isSafeInteger(sequence) ||
    sequence < 1
  )
    return;
  if (event.topic === "job.finished") return { job: event.jobRef, sequence };
  if (event.topic === "target.status_changed" && body?.target?.raw)
    return {
      job: event.jobRef,
      sequence,
      observation: body as TargetProbeEvent,
    };
}

/** Accepts only increasing observations for the current, unfinished probe. */
function accept(window: ProbeWindow, update: ProbeUpdate): boolean {
  if (
    window.finished ||
    update.job !== window.job ||
    update.sequence <= window.sequence ||
    (update.observation && !window.targets.has(update.observation.target.raw))
  ) {
    return false;
  }
  window.sequence = update.sequence;
  window.finished = !update.observation;
  return true;
}

/** Maintains the target list and accepts only newer probes for its active job. */
export function useTargets(client: AnalysisClient) {
  const [list, setList] = useState<TargetList>();
  const [probes, setProbes] = useState<Record<string, TargetProbeEvent>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [finished, setFinished] = useState(false);
  const enabled = useRef(true);
  const terminal = useRef(new Set<string>());
  const pendingLists = useRef(0);
  const probesInFlight = useRef(new Set<string>());
  const active = useRef<ProbeWindow>({
    request: 0,
    job: null,
    sequence: 0,
    finished: false,
    targets: new Set(),
    early: null,
  });

  const refresh = useCallback(
    async (requested: boolean) => {
      const state = active.current;
      enabled.current = true;
      const request = ++state.request;
      state.job = null;
      const earlyUpdates: ProbeUpdate[] = [];
      state.early = earlyUpdates;
      setError(undefined);
      setLoading(true);
      pendingLists.current += 1;
      try {
        const response = await client.call("target.list", {
          refresh: requested,
        });
        if (state.request !== request) {
          if (
            response.probeJobRef &&
            !terminal.current.has(response.probeJobRef)
          )
            void client
              .call("job.cancel", { jobRef: response.probeJobRef })
              .catch(() => {});
          return;
        }
        state.job = response.probeJobRef;
        if (response.probeJobRef && !terminal.current.has(response.probeJobRef))
          probesInFlight.current.add(response.probeJobRef);
        state.sequence = 0;
        state.finished = false;
        state.targets = new Set(response.targets.map(({ path }) => path.raw));
        // An OS probe may finish before the IPC reply identifying its job.
        const early = state.early ?? [];
        state.early = null;
        const observations = early
          .filter((update) => accept(state, update))
          .flatMap(({ observation }) =>
            observation ? [[observation.target.raw, observation] as const] : [],
          );
        setProbes(Object.fromEntries(observations));
        setFinished(state.finished);
        setList(response);
      } catch (error) {
        if (state.request === request) {
          state.early = null;
          setError(
            error instanceof Error ? error.message : "Target listing failed",
          );
        }
      } finally {
        pendingLists.current -= 1;
        if (pendingLists.current === 0) terminal.current.clear();
        if (state.request === request) setLoading(false);
      }
    },
    [client],
  );

  const cancel = useCallback(() => {
    const state = active.current;
    enabled.current = false;
    state.request += 1;
    for (const jobRef of probesInFlight.current)
      void client.call("job.cancel", { jobRef }).catch(() => {});
    probesInFlight.current.clear();
    state.job = null;
    state.early = null;
    state.targets.clear();
    // Keep already observed drives selectable after the draft is discarded.
    setFinished(true);
    setLoading(false);
    setError(undefined);
  }, [client]);

  useEffect(() => {
    const state = active.current;
    const unsubscribe = client.subscribe((event) => {
      if (event.topic === "job.resync_required") {
        if (enabled.current) void refresh(true);
        return;
      }
      const update = decode(event);
      if (!update) return;
      if (!update.observation) {
        if (pendingLists.current > 0) terminal.current.add(update.job);
        probesInFlight.current.delete(update.job);
      }
      if (state.early) {
        if (state.early.length < 1_024) state.early.push(update);
        return;
      }
      if (accept(state, update)) {
        const { observation } = update;
        if (observation)
          setProbes((previous) => ({
            ...previous,
            [observation.target.raw]: observation,
          }));
        else setFinished(true);
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

  return { list, probes, loading, error, finished, refresh, cancel };
}
