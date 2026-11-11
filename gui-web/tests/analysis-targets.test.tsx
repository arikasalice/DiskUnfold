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

test("F01 FR7: probe observations advance independently for each listed target", async () => {
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
  let receive: (event: EventEnvelope) => void = () => undefined;
  const client: AnalysisClient = {
    call: (async () => {
      receive(probe(first, 4, "accessible"));
      receive(probe(second, 3, "denied"));
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
  await waitFor(() => expect(result.current.list).toEqual(list));
  expect(result.current.probes[first.raw]?.access).toBe("accessible");
  expect(result.current.probes[second.raw]?.access).toBe("denied");

  act(() => {
    receive(probe(first, 2, "missing"));
    receive(probe(second, 5, "accessible"));
    receive({ ...probe(first, 4, "accessible"), topic: "job.finished" });
  });
  expect(result.current.probes[first.raw]?.access).toBe("accessible");
  expect(result.current.probes[second.raw]?.access).toBe("accessible");
  expect(result.current.finished).toBe(false);

  act(() => {
    receive({ ...probe(first, 6, "accessible"), topic: "job.finished" });
    receive(probe(first, 7, "missing"));
  });
  expect(result.current.finished).toBe(true);
  expect(result.current.probes[first.raw]?.access).toBe("accessible");
});
