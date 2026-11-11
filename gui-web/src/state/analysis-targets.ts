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
  highestSequence: number;
  targetSequences: Map<string, number>;
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

/** Orders observations per target and the terminal marker across the whole probe job. */
function accept(window: ProbeWindow, update: ProbeUpdate): boolean {
  if (window.finished || update.job !== window.job) return false;
  if (!update.observation) {
    if (update.sequence <= window.highestSequence) return false;
    window.finished = true;
    return true;
  }
  const key = update.observation.target.raw;
  const previous = window.targetSequences.get(key);
  if (previous === undefined || update.sequence <= previous) return false;
  window.targetSequences.set(key, update.sequence);
  window.highestSequence = Math.max(window.highestSequence, update.sequence);
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
  const terminal = useRef(new Map<string, number>());
  const pendingLists = useRef(0);
  const probesInFlight = useRef(new Set<string>());
  const active = useRef<ProbeWindow>({
    request: 0,
    job: null,
    highestSequence: 0,
    finished: false,
    targetSequences: new Map(),
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
        state.highestSequence = 0;
        state.finished = false;
        state.targetSequences = new Map(
          response.targets.map(({ path }) => [path.raw, 0]),
        );
        // An OS probe may finish before the IPC reply identifying its job.
        const early = state.early ?? [];
        state.early = null;
        const observations = early
          .filter((update) => accept(state, update))
          .flatMap(({ observation }) =>
            observation ? [[observation.target.raw, observation] as const] : [],
          );
        if (state.job) {
          // The bounded observation buffer may have filled before the terminal arrived.
          const sequence = terminal.current.get(state.job);
          if (sequence !== undefined)
            accept(state, { job: state.job, sequence });
          if (state.finished) probesInFlight.current.delete(state.job);
          else probesInFlight.current.add(state.job);
        }
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
    state.targetSequences.clear();
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
      if (
        !update.observation &&
        (update.job !== state.job || update.sequence > state.highestSequence)
      ) {
        if (pendingLists.current > 0) {
          const previous = terminal.current.get(update.job) ?? 0;
          terminal.current.set(update.job, Math.max(previous, update.sequence));
        }
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
