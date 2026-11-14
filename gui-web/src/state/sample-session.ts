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

/** True when a response must carry the sample badge: a missing source counts as a sample. */
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
  const held = useRef<SampleSession>(undefined);
  const busy = useRef(false);
  const read = useRef(0);
  const sawEvent = useRef(false);

  const store = useCallback((next: SampleSession | undefined) => {
    held.current = next;
    setSession(next);
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
      if (event.topic !== "sample.state_changed") return;
      const body = readSampleEvent(event.body);
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

  return useMemo(
    () => ({ session, pending, problem, open, reset, close, dismiss }),
    [session, pending, problem, open, reset, close, dismiss],
  );
}
