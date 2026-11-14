import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnalysisError } from "../ipc";
import type {
  AnalysisClient,
  DataSource,
  EventEnvelope,
  SampleEventState,
  SampleSession,
  SampleSessionState,
  SampleStateChanged,
} from "../ipc";

/** The command whose request is in flight; a second request is not sent meanwhile. */
export type SampleCommand = "open" | "reset" | "close";
/** What the user needs to be told about a failed sample command. */
export type SampleProblem = "busy" | "closed" | "failed";

/**
 * True when a response must carry the sample badge: a missing source counts as
 * a sample, as the contract rule requires. The engine's browse.detail answer
 * is expected to fill dataSource every time; whether it does is checked on the
 * Rust side, and the screen keeps this rule until that is confirmed.
 */
export function isSampleSource(source: DataSource | undefined): boolean {
  return source !== "real";
}

/** True while the session still owns sample data (anything but closed). */
export function isOpenSample(
  session: SampleSession | undefined,
): session is SampleSession {
  return session !== undefined && session.state !== "closed";
}

/**
 * True while the sample result screen owns the main area: an active session,
 * or an active one that is being prepared again after a reset. Before the
 * first activation, and while the sample is unavailable, the real analysis
 * screen stays and the sample state is shown at the entry button.
 */
export function isSampleScreen(
  session: SampleSession | undefined,
  reached: boolean,
): session is SampleSession {
  if (session === undefined) return false;
  if (session.state === "active") return true;
  return (
    reached && (session.state === "preparing" || session.state === "resetting")
  );
}

/**
 * Applies a status, open or reset response. A response for the held session
 * with a lower sequence is older than what the store already shows.
 */
export function acceptSession(
  held: SampleSession | undefined,
  incoming: SampleSession,
): SampleSession {
  if (
    held?.sessionRef === incoming.sessionRef &&
    incoming.sequence < held.sequence
  )
    return held;
  return incoming;
}

const eventStates = new Set<string>([
  "inactive",
  "loading",
  "active",
  "failed",
]);

/** Reads an event body, or returns nothing when it cannot be trusted. */
export function readSampleEvent(body: unknown): SampleStateChanged | undefined {
  if (typeof body !== "object" || body === null) return undefined;
  const value = body as Partial<SampleStateChanged>;
  if (
    typeof value.sessionRef !== "string" ||
    typeof value.state !== "string" ||
    !eventStates.has(value.state) ||
    typeof value.sequence !== "number" ||
    !Number.isSafeInteger(value.sequence)
  )
    return undefined;
  return value as SampleStateChanged;
}

/** Reads the number of items a job.progress body reports, or nothing when it cannot be trusted. */
export function readItemProgress(
  body: unknown,
): { sequence: number; completed: number } | undefined {
  if (typeof body !== "object" || body === null) return undefined;
  const value = body as {
    unit?: unknown;
    completed?: unknown;
    sequence?: unknown;
  };
  if (
    value.unit !== "items" ||
    typeof value.completed !== "number" ||
    !Number.isSafeInteger(value.completed) ||
    typeof value.sequence !== "number" ||
    !Number.isSafeInteger(value.sequence)
  )
    return undefined;
  return { sequence: value.sequence, completed: value.completed };
}

type Prepared = { jobRef: string; sequence: number; completed: number };

/**
 * Follows the item count of the sample's loading job. A progress event counts
 * only for the job the last state event named, and only when it is newer than
 * the count held.
 */
export function followPrepared(
  held: Prepared | undefined,
  loadingJob: string | undefined,
  event: EventEnvelope,
): Prepared | undefined {
  const progress = readItemProgress(event.body);
  if (!progress || event.jobRef === undefined || event.jobRef !== loadingJob)
    return held;
  if (held?.jobRef === loadingJob && held.sequence >= progress.sequence)
    return held;
  return { jobRef: loadingJob, ...progress };
}

function sessionState(
  event: SampleEventState,
  held: SampleSessionState,
): SampleSessionState {
  if (event === "active") return "active";
  if (event === "failed") return "unavailable";
  if (event === "inactive") return "closed";
  return held === "resetting" ? "resetting" : "preparing";
}

/**
 * Applies one relayed state event. Events at or below the stored sequence are
 * dropped. The event carries only the coarse state, so the caller reads the
 * detail with sample.status afterwards.
 */
export function applySampleEvent(
  held: SampleSession | undefined,
  event: SampleStateChanged,
): SampleSession | undefined {
  if (held?.sessionRef !== event.sessionRef || event.sequence <= held.sequence)
    return undefined;
  const next: SampleSession = {
    ...held,
    state: sessionState(event.state, held.state),
    sequence: event.sequence,
  };
  delete next.unavailableReason;
  if (event.state === "failed" && event.reasonKey !== undefined)
    next.unavailableReason = event.reasonKey;
  return next;
}

function failureOf(error: unknown): SampleProblem {
  if (!(error instanceof AnalysisError)) return "failed";
  if (error.code === "NOT_FOUND" && error.reasonKey === "sample.session_closed")
    return "closed";
  if (error.code === "BUSY" || error.reasonKey === "job.queue_full")
    return "busy";
  return "failed";
}

/**
 * Holds the sample experience session of the window. The store follows
 * sample.status responses and relayed sample.state_changed events only, and
 * never reuses state from the real analysis.
 */
export function useSampleSession(client: AnalysisClient | undefined) {
  const [session, setSession] = useState<SampleSession>();
  const [pending, setPending] = useState<SampleCommand>();
  const [problem, setProblem] = useState<SampleProblem>();
  const [reachedRef, setReachedRef] = useState<string>();
  const [prepared, setPrepared] = useState<Prepared>();
  const held = useRef<SampleSession>(undefined);
  const loadingJob = useRef<string>(undefined);
  const busy = useRef(false);
  const read = useRef(0);
  const sawEvent = useRef(false);

  const store = useCallback((next: SampleSession | undefined) => {
    held.current = next;
    setSession(next);
    // The screen is reached by the first activation and lost by a failure or a release.
    if (next?.state === "active") setReachedRef(next.sessionRef);
    else if (next === undefined || next.state === "unavailable")
      setReachedRef(undefined);
    if (next?.state !== "preparing" && next?.state !== "resetting")
      setPrepared(undefined);
  }, []);

  const accept = useCallback(
    (incoming: SampleSession) => {
      if (incoming.state === "closed") {
        store(undefined);
        setProblem("closed");
        return;
      }
      const next = acceptSession(held.current, incoming);
      if (next !== held.current) store(next);
    },
    [store],
  );

  const fail = useCallback(
    (error: unknown) => {
      const kind = failureOf(error);
      if (kind === "closed") store(undefined);
      setProblem(kind);
    },
    [store],
  );

  const refresh = useCallback(async () => {
    const current = held.current;
    if (!client || !current) return;
    const request = ++read.current;
    try {
      const next = await client.call("sample.status", {
        sessionRef: current.sessionRef,
      });
      if (
        request === read.current &&
        held.current?.sessionRef === current.sessionRef
      )
        accept(next);
    } catch (error) {
      if (
        request === read.current &&
        held.current?.sessionRef === current.sessionRef
      )
        fail(error);
    }
  }, [accept, client, fail]);

  const watching = session !== undefined || pending === "open";
  useEffect(() => {
    if (!client || !watching) return;
    return client.subscribe((event: EventEnvelope) => {
      if (event.topic === "job.resync_required") {
        void refresh();
        return;
      }
      if (event.topic === "job.progress") {
        setPrepared((before) =>
          followPrepared(before, loadingJob.current, event),
        );
        return;
      }
      if (event.topic !== "sample.state_changed") return;
      const body = readSampleEvent(event.body);
      if (body?.jobRef !== undefined) loadingJob.current = body.jobRef;
      if (!held.current) {
        sawEvent.current = true;
        return;
      }
      if (!body) {
        void refresh();
        return;
      }
      const next = applySampleEvent(held.current, body);
      if (!next) return;
      store(next);
      void refresh();
    });
  }, [client, refresh, store, watching]);

  const run = useCallback(
    async <T>(command: SampleCommand, action: () => Promise<T>) => {
      if (!client || busy.current) return undefined;
      busy.current = true;
      setPending(command);
      setProblem(undefined);
      try {
        return await action();
      } finally {
        busy.current = false;
        setPending(undefined);
      }
    },
    [client],
  );

  const open = useCallback(
    () =>
      run("open", async () => {
        if (!client) return;
        sawEvent.current = false;
        try {
          const current = held.current;
          accept(
            isOpenSample(current)
              ? await client.call("sample.status", {
                  sessionRef: current.sessionRef,
                })
              : await client.call("sample.open", {}),
          );
          if (sawEvent.current) void refresh();
        } catch (error) {
          fail(error);
        }
      }),
    [accept, client, fail, refresh, run],
  );

  const reset = useCallback(
    () =>
      run("reset", async () => {
        const current = held.current;
        if (!client || !current) return;
        try {
          accept(
            await client.call("sample.reset", {
              sessionRef: current.sessionRef,
            }),
          );
        } catch (error) {
          fail(error);
        }
      }),
    [accept, client, fail, run],
  );

  /** Resolves true once the session no longer exists, so the caller may leave the sample. */
  const close = useCallback(
    async (): Promise<boolean> =>
      (await run("close", async () => {
        const current = held.current;
        if (!client || !current) return true;
        try {
          await client.call("sample.close", { sessionRef: current.sessionRef });
        } catch (error) {
          // A failed release still ends the session on the host side.
          const internal =
            error instanceof AnalysisError && error.code === "INTERNAL";
          if (!internal) {
            fail(error);
            return held.current === undefined;
          }
        }
        read.current += 1;
        store(undefined);
        return true;
      })) === true,
    [client, fail, run, store],
  );

  const dismiss = useCallback(() => setProblem(undefined), []);

  const reached = session !== undefined && session.sessionRef === reachedRef;
  return useMemo(
    () => ({
      session,
      /** True once the held session has been active and has not failed since. */
      reached,
      /** Example items prepared so far while the sample loads; unknown when the host sends no count. */
      preparedItems: prepared?.completed,
      pending,
      problem,
      open,
      reset,
      close,
      dismiss,
    }),
    [session, reached, prepared, pending, problem, open, reset, close, dismiss],
  );
}
