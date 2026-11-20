import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { ScanLane, ScanLaneStatus } from "../src/ipc";
import { useScanPriority } from "../src/state/scan-priority";
import type { ScanPriorityContext } from "../src/state/scan-priority";
import { priorityClient, ref, running } from "./scan-priority-fixture";

afterEach(cleanup);

function laneStatus(
  sequence: number,
  priorityArea?: ScanLane["priorityArea"],
): ScanLaneStatus {
  return {
    sequence,
    observedAt: "2026-11-20T09:00:00.000Z",
    lanes: [
      {
        laneIndex: 0,
        rootIndexes: [0],
        inFlight: 1,
        limit: 1,
        waitReason: "none",
        waitConfirmed: true,
        remainingDirectories: "1",
        failures: "0",
        ...(priorityArea ? { priorityArea } : {}),
      },
    ],
  };
}

test("the row is requested until IF-08 reports its ItemRef, then analyzing", () => {
  const { client } = priorityClient();
  const context: ScanPriorityContext = { ...running };
  const hook = renderHook(
    ({ context }: { context: ScanPriorityContext }) =>
      useScanPriority(client, context),
    { initialProps: { context } },
  );
  act(() => hook.result.current.request(ref(1), "Photos"));
  expect(hook.result.current.status(ref(1))).toBe("requested");
  expect(hook.result.current.requestedArea?.name).toBe("Photos");

  hook.rerender({
    context: { ...running, laneStatus: laneStatus(1, ref(1)) },
  });
  expect(hook.result.current.status(ref(1))).toBe("analyzing");

  hook.rerender({ context: { ...running, laneStatus: undefined } });
  expect(hook.result.current.status(ref(1))).toBe("requested");
});

test("a completed reported area releases the request mark", async () => {
  const { client } = priorityClient();
  const context: ScanPriorityContext = { ...running };
  const hook = renderHook(
    ({ context }: { context: ScanPriorityContext }) =>
      useScanPriority(client, context),
    { initialProps: { context } },
  );
  act(() => hook.result.current.request(ref(1), "Photos"));
  hook.rerender({
    context: { ...running, laneStatus: laneStatus(1, ref(1)) },
  });
  expect(hook.result.current.status(ref(1))).toBe("analyzing");

  hook.rerender({ context: { ...running, laneStatus: laneStatus(2) } });
  await waitFor(() => {
    expect(hook.result.current.status(ref(1))).toBeUndefined();
    expect(hook.result.current.requestedArea).toBeUndefined();
  });
});

test("IF-08 cannot supply a name after reload or for another ItemRef", () => {
  const { client } = priorityClient();
  const context: ScanPriorityContext = {
    ...running,
    laneStatus: laneStatus(1, ref(1)),
  };
  const hook = renderHook(() => useScanPriority(client, context));
  expect(hook.result.current.status(ref(1))).toBeUndefined();
  expect(hook.result.current.requestedArea).toBeUndefined();

  act(() => hook.result.current.request(ref(2), "Videos"));
  expect(hook.result.current.status(ref(2))).toBe("requested");
  expect(hook.result.current.status(ref(1))).toBeUndefined();
});
