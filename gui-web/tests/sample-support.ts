import type {
  AnalysisClient,
  EventEnvelope,
  SampleSession,
  SampleStateChanged,
} from "../src/ipc";

export const sampleSession = (
  over: Partial<SampleSession> = {},
): SampleSession => ({
  sessionRef: "sample-1",
  state: "active",
  sequence: 1,
  bundleVersion: "2026.1",
  observations: [
    { index: 0, resultVersion: "rv0", observedAt: "2026-01-01T00:00:00Z" },
    { index: 1, resultVersion: "rv1", observedAt: "2026-02-01T00:00:00Z" },
  ],
  plans: [],
  ...over,
});

export function sampleEvent(
  topic: string,
  body: Partial<SampleStateChanged> | undefined,
): EventEnvelope {
  return {
    topic,
    seq: 1,
    contractVersion: 1,
    occurredAt: "2026-02-01T00:00:00Z",
    body,
  };
}

type Handler = (body: unknown) => unknown;

/** A fake client whose commands are answered by `handlers`; unknown commands fail. */
export function sampleClient(handlers: Record<string, Handler>) {
  const calls: { name: string; body: unknown }[] = [];
  const listeners = new Set<(event: EventEnvelope) => void>();
  const client: AnalysisClient = {
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    call: ((name: string, body: unknown) => {
      calls.push({ name, body });
      const handler = handlers[name];
      return handler
        ? Promise.resolve().then(() => handler(body))
        : Promise.reject(new Error(name));
    }) as AnalysisClient["call"],
  };
  return {
    client,
    calls,
    count: (name: string) => calls.filter((call) => call.name === name).length,
    emit: (event: EventEnvelope) => listeners.forEach((fn) => fn(event)),
  };
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}
