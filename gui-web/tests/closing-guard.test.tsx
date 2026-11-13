import axe from "axe-core";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ConfirmDialog } from "../src/components";
import { ClosingGuard } from "../src/features/shutdown/ClosingGuard";
import { readClosingSignal } from "../src/features/shutdown/lifecycle";
import { AnalysisError } from "../src/ipc";
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
    screen.getByRole("progressbar", { name: "Closing progress" }),
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
    "DiskUnfold has finished closing.",
  );
  expect(
    screen.queryByRole("button", { name: "Quit without waiting" }),
  ).toBeNull();
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

const closingEvent = (
  sequence: number,
  state: "access_closed" | "draining",
  remaining: number,
) =>
  lifecycle(sequence, {
    state,
    previousState: state === "draining" ? "access_closed" : "running",
    remainingJobCount: remaining,
  });

function renderClosing(call?: Parameters<typeof fixture>[0]) {
  const harness = fixture(call);
  render(
    <ClosingGuard client={harness.client}>
      <button type="button">workspace action</button>
    </ClosingGuard>,
  );
  return harness;
}

const screenRegion = () =>
  screen.getByRole("region", { name: "Closing DiskUnfold" });
const quitButton = () =>
  screen.getByRole("button", { name: "Quit without waiting" });

test("the closing screen shows the phase and the remaining count and takes focus", async () => {
  const { emit } = renderClosing();
  await emit(closingEvent(1, "access_closed", 3));
  expect(
    screen.getByRole("heading", { name: "Closing DiskUnfold" }),
  ).toBeTruthy();
  expect(screen.getByRole("status").textContent).toBe(
    "Waiting for 3 jobs to finish.",
  );
  expect(
    screen.getByText("Stopping new requests and asking running jobs to stop."),
  ).toBeTruthy();
  expect(document.activeElement).toBe(screenRegion());
  expect(quitButton()).toBeTruthy();
  // The workspace is not rendered at all, so nothing behind the screen can be reached.
  expect(document.body.textContent).not.toContain("workspace action");
  expect(document.querySelector("[inert]")).toBeNull();
  // One name for the heading and a different one for the progress bar.
  expect(
    screen.getByRole("progressbar", { name: "Closing progress" }),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: /cancel|keep/i })).toBeNull();

  await emit(closingEvent(2, "draining", 1));
  expect(screen.getByRole("status").textContent).toBe(
    "Waiting for 1 job to finish.",
  );
  expect(quitButton()).toBeTruthy();
  expect((await axe.run(screenRegion())).violations).toEqual([]);
});

test("quit without waiting sends the unconfirmed mode and then reports that it is leaving", async () => {
  const { emit, call } = renderClosing();
  await emit(closingEvent(1, "draining", 2));
  fireEvent.click(quitButton());
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith("app.quit", { mode: "without_waiting" }),
  );
  const leaving = await screen.findByText(/Quitting without waiting/);
  expect(leaving.textContent).toContain("not recorded as complete");
  expect((quitButton() as HTMLButtonElement).disabled).toBe(true);
  expect(document.activeElement).toBe(screenRegion());
  expect((await axe.run(screenRegion())).violations).toEqual([]);
});

test("an already closing answer to quit without waiting is shown as leaving", async () => {
  const call = vi.fn().mockResolvedValue({ outcome: "already_closing" });
  const { emit } = renderClosing(call);
  await emit(closingEvent(1, "draining", 2));
  fireEvent.click(quitButton());
  expect(await screen.findByText(/Quitting without waiting/)).toBeTruthy();
});

test("confirm_required opens a confirmation and the confirmed request repeats without_waiting", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce({
      outcome: "confirm_required",
      inProgressChangeCount: 2,
    })
    .mockResolvedValueOnce({ outcome: "already_closing" });
  const { emit } = renderClosing(call);
  await emit(closingEvent(1, "draining", 3));
  fireEvent.click(quitButton());
  const dialog = await screen.findByRole("alertdialog");
  expect(call).toHaveBeenCalledTimes(1);
  expect(call).toHaveBeenLastCalledWith("app.quit", {
    mode: "without_waiting",
  });
  expect(dialog.textContent).toContain(
    "Quit without waiting while file changes are running?",
  );
  expect(dialog.textContent).toContain("2 file changes are still running");
  expect(dialog.textContent).toContain("the remaining items are not processed");
  expect((await axe.run(dialog)).violations).toEqual([]);

  // Progress keeps updating behind the open confirmation.
  await emit(closingEvent(2, "draining", 2));
  expect(screen.getByRole("alertdialog")).toBeTruthy();

  fireEvent.click(
    within(screen.getByRole("alertdialog")).getByRole("button", {
      name: "Quit without waiting",
    }),
  );
  await waitFor(() => expect(call).toHaveBeenCalledTimes(2));
  expect(call).toHaveBeenLastCalledWith("app.quit", {
    mode: "without_waiting",
    confirmUnfinishedChanges: true,
  });
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(await screen.findByText(/Quitting without waiting/)).toBeTruthy();
});

test("cancelling the confirmation sends nothing more and returns focus to the button", async () => {
  const call = vi.fn().mockResolvedValue({
    outcome: "confirm_required",
    inProgressChangeCount: 1,
  });
  const { emit } = renderClosing(call);
  await emit(closingEvent(1, "draining", 1));
  // A mouse press focuses the button first, as in a browser.
  act(() => quitButton().focus());
  press(quitButton());
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog.contains(document.activeElement)).toBe(true);
  expect(dialog.textContent).toContain("1 file change is still running");
  fireEvent.click(screen.getByRole("button", { name: "Keep waiting" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(call).toHaveBeenCalledTimes(1);
  expect(screen.queryByText(/Quitting without waiting/)).toBeNull();
  expect((quitButton() as HTMLButtonElement).disabled).toBe(false);
  // Nothing in the test moves focus back; the dialog restores it to the opener.
  await waitFor(() => expect(document.activeElement).toBe(quitButton()));
});

test("a refusal after acceptance says it is already closing and a failure offers a retry", async () => {
  const call = vi
    .fn()
    .mockRejectedValueOnce(
      new AnalysisError("INVALID_ARGUMENT", "startup.shutting_down", false),
    )
    .mockRejectedValueOnce(new Error("transport"))
    .mockResolvedValueOnce({ outcome: "future_outcome" });
  const { emit } = renderClosing(call);
  await emit(closingEvent(1, "draining", 1));

  fireEvent.click(quitButton());
  expect(
    await screen.findByText("DiskUnfold is already closing."),
  ).toBeTruthy();
  expect(screen.queryByRole("alert")).toBeNull();

  fireEvent.click(quitButton());
  expect((await screen.findByRole("alert")).textContent).toContain("Try again");
  expect(screen.queryByText("DiskUnfold is already closing.")).toBeNull();

  fireEvent.click(quitButton());
  await waitFor(() => expect(call).toHaveBeenCalledTimes(3));
  expect((await screen.findByRole("alert")).textContent).toContain("Try again");
  expect((quitButton() as HTMLButtonElement).disabled).toBe(false);
});

test("the closed phase drops the quit button and ends the confirmation", async () => {
  const call = vi.fn().mockResolvedValue({
    outcome: "confirm_required",
    inProgressChangeCount: 1,
  });
  const { emit } = renderClosing(call);
  await emit(closingEvent(1, "draining", 1));
  fireEvent.click(quitButton());
  await screen.findByRole("alertdialog");
  await emit(
    lifecycle(2, {
      state: "closed",
      previousState: "draining",
      remainingJobCount: 0,
    }),
  );
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(
    screen.queryByRole("button", { name: "Quit without waiting" }),
  ).toBeNull();
  expect(screen.getByRole("status").textContent).toBe(
    "DiskUnfold has finished closing.",
  );
});

/** A pointer press the way a user makes it: focus lands on the button first. */
function press(element: HTMLElement) {
  fireEvent.pointerDown(element, { pointerType: "mouse", button: 0 });
  fireEvent.mouseDown(element, { button: 0 });
  fireEvent.pointerUp(element, { pointerType: "mouse", button: 0 });
  fireEvent.mouseUp(element, { button: 0 });
  fireEvent.click(element, { button: 0 });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

test("a workspace dialog open when closing starts is removed and focus moves to the closing screen", async () => {
  const harness = fixture();
  render(
    <ClosingGuard client={harness.client}>
      <ConfirmDialog
        title="Apply the plan?"
        confirmLabel="Apply"
        cancelLabel="Back"
        onConfirm={() => {}}
        onCancel={() => {}}
      >
        <p>plan details</p>
      </ConfirmDialog>
    </ClosingGuard>,
  );
  expect(await screen.findByRole("alertdialog")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Apply" })).toBeTruthy();

  await harness.emit(closingEvent(1, "draining", 1));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(screen.queryByRole("button", { name: "Apply" })).toBeNull();
  expect(document.body.textContent).not.toContain("plan details");
  await waitFor(() => expect(document.activeElement).toBe(screenRegion()));
  expect(quitButton()).toBeTruthy();
});

test("quit without waiting sends one request even when pressed twice", async () => {
  const reply = deferred<{ outcome: string }>();
  const call = vi.fn().mockReturnValue(reply.promise);
  const { emit } = renderClosing(call);
  await emit(closingEvent(1, "draining", 1));
  fireEvent.click(quitButton());
  fireEvent.click(quitButton());
  expect(call).toHaveBeenCalledTimes(1);
  await act(async () => reply.resolve({ outcome: "accepted" }));
  expect(await screen.findByText(/Quitting without waiting/)).toBeTruthy();
  expect(call).toHaveBeenCalledTimes(1);
});

test("a confirm_required reply that arrives after the closed event opens no dialog", async () => {
  const reply = deferred<{ outcome: string; inProgressChangeCount: number }>();
  const call = vi.fn().mockReturnValue(reply.promise);
  const { emit } = renderClosing(call);
  await emit(closingEvent(1, "draining", 1));
  fireEvent.click(quitButton());
  await emit(
    lifecycle(2, {
      state: "closed",
      previousState: "draining",
      remainingJobCount: 0,
    }),
  );
  await act(async () =>
    reply.resolve({ outcome: "confirm_required", inProgressChangeCount: 1 }),
  );
  expect(screen.queryByRole("alertdialog")).toBeNull();
  expect(screen.getByRole("status").textContent).toBe(
    "DiskUnfold has finished closing.",
  );
});

test("cancelling after a failed confirmation leaves no stale alert", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce({
      outcome: "confirm_required",
      inProgressChangeCount: 1,
    })
    .mockRejectedValueOnce(new Error("transport"));
  const { emit } = renderClosing(call);
  await emit(closingEvent(1, "draining", 1));
  fireEvent.click(quitButton());
  const dialog = await screen.findByRole("alertdialog");
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Quit without waiting" }),
  );
  expect(await screen.findAllByRole("alert")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Keep waiting" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(screen.queryByRole("alert")).toBeNull();
});

test("a shutting_down refusal before any closing event still shows already closing", async () => {
  const call = vi
    .fn()
    .mockRejectedValue(
      new AnalysisError("INVALID_ARGUMENT", "startup.shutting_down", false),
    );
  const { emit } = renderClosing(call);
  await emit(request(1));
  fireEvent.click(
    await screen.findByRole("button", { name: "Close and stop changes" }),
  );
  expect(
    await screen.findByText("DiskUnfold is already closing."),
  ).toBeTruthy();
  expect(screenRegion()).toBeTruthy();
});

test("the live region starts empty and is then filled", async () => {
  const { emit } = renderClosing();
  await emit(closingEvent(1, "draining", 2));
  expect(screen.getByRole("status").textContent).toBe(
    "Waiting for 2 jobs to finish.",
  );
});
