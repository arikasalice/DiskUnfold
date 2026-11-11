import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  EventEnvelope,
  TargetList,
  TargetProbeEvent,
} from "../src/ipc";
import { useTargets } from "../src/state/analysis-targets";

afterEach(cleanup);

const first = windowsPath("C:\\");
const second = windowsPath("D:\\");
const observedAt = "2026-11-11T00:00:00.000Z";
const probe = (
  target: typeof first,
  sequence: number,
  access: TargetProbeEvent["access"],
) => ({
  topic: "target.status_changed",
  contractVersion: 1,
  seq: sequence,
  occurredAt: observedAt,
  jobRef: "probe",
  body: {
    target,
    sequence,
    access,
    reasonKey: null,
    totalBytes: null,
    availableBytes: null,
    capacityReason: null,
    aggregationUncertain: true,
    observedAt,
  } satisfies TargetProbeEvent,
});
const list: TargetList = {
  targets: [first, second].map((path) => ({
    kind: "internal",
    path,
    connected: true,
    volumeId: null,
    identitySource: "unavailable",
    access: "checking",
    reasonKey: null,
  })),
  probeJobRef: "probe",
  observedAt,
  unavailableReason: null,
};
function fixture(early: (emit: (event: EventEnvelope) => void) => void) {
  let receive: (event: EventEnvelope) => void = () => undefined;
  const cancelled: unknown[] = [];
  const client: AnalysisClient = {
    call: (async (name: string, body: unknown) => {
      if (name === "job.cancel") {
        cancelled.push(body);
        return;
      }
      early((event) => receive(event));
      return list;
    }) as AnalysisClient["call"],
    subscribe(listener) {
      receive = listener;
      return () => {
        receive = () => undefined;
      };
    },
  };
  const { result } = renderHook(() => useTargets(client));
  return {
    result,
    list,
    cancelled,
    emit: (event: EventEnvelope) => receive(event),
  };
}

test("F01 FR7: probe observations advance independently for each listed target", async () => {
  const { result, list, emit } = fixture((receive) => {
    receive(probe(first, 4, "accessible"));
    receive(probe(second, 3, "denied"));
  });
  await waitFor(() => expect(result.current.list).toEqual(list));
  expect(result.current.probes[first.raw]?.access).toBe("accessible");
  expect(result.current.probes[second.raw]?.access).toBe("denied");

  act(() => {
    emit(probe(first, 2, "missing"));
    emit(probe(second, 5, "accessible"));
    emit({ ...probe(first, 4, "accessible"), topic: "job.finished" });
  });
  expect(result.current.probes[first.raw]?.access).toBe("accessible");
  expect(result.current.probes[second.raw]?.access).toBe("accessible");
  expect(result.current.finished).toBe(false);

  act(() => {
    emit({ ...probe(first, 6, "accessible"), topic: "job.finished" });
    emit(probe(first, 7, "missing"));
  });
  expect(result.current.finished).toBe(true);
  expect(result.current.probes[first.raw]?.access).toBe("accessible");
});

test.each([
  { buffered: 1, terminal: 3, finished: false },
  { buffered: 1_024, terminal: 5, finished: true },
])(
  "F01 FR7: early terminal $terminal after $buffered observations",
  async ({ buffered, terminal, finished }) => {
    const { result, list, cancelled } = fixture((emit) => {
      const observation = probe(first, 4, "accessible");
      for (let index = 0; index < buffered; index++) emit(observation);
      emit({ ...probe(first, terminal, "accessible"), topic: "job.finished" });
    });
    await waitFor(() => expect(result.current.list).toEqual(list));
    expect(result.current.finished).toBe(finished);
    expect(result.current.probes[first.raw]?.access).toBe("accessible");
    act(() => result.current.cancel());
    expect(cancelled).toEqual(finished ? [] : [{ jobRef: "probe" }]);
  },
);
