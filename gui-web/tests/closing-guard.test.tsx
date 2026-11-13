import axe from "axe-core";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ClosingGuard } from "../src/features/shutdown/ClosingGuard";
import { readClosingSignal } from "../src/features/shutdown/lifecycle";
import type { AnalysisClient, EventEnvelope } from "../src/ipc";

afterEach(cleanup);

function lifecycle(
  sequence: number,
  body: Record<string, unknown>,
): EventEnvelope {
  return {
    topic: "app.lifecycle_changed",
    contractVersion: 1,
    seq: sequence,
    occurredAt: "2026-11-13T00:00:00.000Z",
    body: {
      state: "running",
      previousState: "running",
      remainingJobCount: 1,
      stateSequence: sequence,
      observedAt: "2026-11-13T00:00:00.000Z",
      ...body,
    },
  } as EventEnvelope;
}

function fixture(call = vi.fn().mockResolvedValue({ outcome: "accepted" })) {
  const listeners = new Set<(event: EventEnvelope) => void>();
  const client = {
    call,
    subscribe(listener: (event: EventEnvelope) => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  } as unknown as AnalysisClient;
  return {
    call,
    client,
    emit: (event: EventEnvelope) =>
      act(() => listeners.forEach((listener) => listener(event))),
    subscribers: () => listeners.size,
  };
}

const request = (sequence: number, changes = 2) =>
  lifecycle(sequence, {
    confirmationRequired: true,
    inProgressChangeCount: changes,
    remainingJobCount: changes + 1,
  });

test("asks before leaving running file changes and sends nothing on keep open", async () => {
  const { client, emit, call } = fixture();
  render(
    <ClosingGuard client={client}>
      <p>workspace</p>
    </ClosingGuard>,
  );
  await emit(lifecycle(1, {}));
  expect(screen.queryByRole("alertdialog")).toBeNull();

  await emit(request(2));
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog.textContent).toContain("2 file changes are still running");
  expect((await axe.run(dialog)).violations).toEqual([]);

  fireEvent.click(screen.getByRole("button", { name: "Keep DiskUnfold open" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(call).not.toHaveBeenCalled();
  expect(screen.getByText("workspace")).toBeTruthy();
});

test("confirming sends the confirmed normal quit and then shows inline progress", async () => {
  const { client, emit, call } = fixture();
  render(
    <ClosingGuard client={client}>
      <p>workspace</p>
    </ClosingGuard>,
  );
  await emit(request(1, 1));
  expect((await screen.findByRole("alertdialog")).textContent).toContain(
    "1 file change is still running",
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Close and stop changes" }),
  );
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith("app.quit", {
      mode: "normal",
      confirmUnfinishedChanges: true,
    }),
  );
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  await emit(
    lifecycle(2, { state: "draining", previousState: "access_closed" }),
  );
  expect(
    screen.getByRole("progressbar", { name: "Closing DiskUnfold" }),
  ).toBeTruthy();
  expect(screen.getByRole("status").textContent).toBe(
    "Waiting for 1 job to finish.",
  );
  await emit(
    lifecycle(3, {
      state: "closed",
      previousState: "draining",
      remainingJobCount: 0,
    }),
  );
  expect(screen.getByRole("status").textContent).toBe(
    "Waiting for 0 jobs to finish.",
  );
});

test("a failed quit keeps the dialog open with an error and can be retried", async () => {
  const call = vi
    .fn()
    .mockRejectedValueOnce(new Error("transport"))
    .mockResolvedValueOnce({ outcome: "accepted" });
  const { client, emit } = fixture(call);
  render(
    <ClosingGuard client={client}>
      <p>workspace</p>
    </ClosingGuard>,
  );
  await emit(request(1));
  fireEvent.click(
    await screen.findByRole("button", { name: "Close and stop changes" }),
  );
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(screen.getByRole("alertdialog")).toBeTruthy();
  fireEvent.click(
    screen.getByRole("button", { name: "Close and stop changes" }),
  );
  await waitFor(() => expect(call).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
});

test("stale or unreadable lifecycle events change nothing and the subscription is released", async () => {
  const { client, emit, subscribers } = fixture();
  const view = render(
    <ClosingGuard client={client}>
      <p>workspace</p>
    </ClosingGuard>,
  );
  await emit(lifecycle(5, { state: "draining" }));
  await emit(request(4));
  expect(screen.queryByRole("alertdialog")).toBeNull();
  await emit(lifecycle(6, { state: "future_state" }));
  await emit({ ...lifecycle(7, {}), body: "not an object" } as EventEnvelope);
  expect(screen.getAllByRole("status")).toHaveLength(1);
  expect(subscribers()).toBe(1);
  view.unmount();
  expect(subscribers()).toBe(0);
});

test("reads only well-formed lifecycle bodies", () => {
  expect(readClosingSignal(request(3, 4))).toEqual({
    kind: "confirm",
    changeCount: 4,
    sequence: 3,
  });
  expect(
    readClosingSignal(lifecycle(3, { confirmationRequired: true })),
  ).toEqual({
    kind: "ignore",
  });
  expect(readClosingSignal(lifecycle(3, { inProgressChangeCount: 2 }))).toEqual(
    {
      kind: "ignore",
    },
  );
  expect(readClosingSignal(lifecycle(3, { stateSequence: -1 }))).toEqual({
    kind: "ignore",
  });
  expect(
    readClosingSignal({ ...lifecycle(3, {}), topic: "job.finished" }),
  ).toEqual({ kind: "ignore" });
});
