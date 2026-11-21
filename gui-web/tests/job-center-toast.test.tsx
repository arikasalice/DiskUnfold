import { useRef, useState } from "react";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { useJobCenterParts } from "../src/features/jobs/JobCenter";
import type {
  AnalysisClient,
  AnalysisJobListRow,
  EventEnvelope,
} from "../src/ipc";

afterEach(cleanup);

const job: AnalysisJobListRow = {
  jobRef: "019a0000-0000-7000-8000-000000000701",
  kind: "scan.metadata",
  scopeSummary: "metadata scan · 1 root",
  scopeFirstRoot: "C:\\Data",
  scopeRootCount: 1,
  state: "running",
  waitReason: "none",
  priority: "normal",
  supportedControls: ["pause", "cancel"],
  sequence: 1,
};

function clientFor() {
  let listener: ((value: EventEnvelope) => void) | undefined;
  const client = {
    subscribe(next: (value: EventEnvelope) => void) {
      listener = next;
      return () => {
        listener = undefined;
      };
    },
    async call(name: string) {
      if (name === "job.list") return [job];
      throw new Error(`Unexpected command ${name}`);
    },
  } as unknown as AnalysisClient;
  return {
    client,
    finish() {
      listener?.({
        topic: "job.finished",
        contractVersion: 1,
        seq: 2,
        occurredAt: "2026-11-21T12:00:00.000Z",
        jobRef: job.jobRef,
        body: {
          state: "done",
          sequence: 2,
          completed: 3,
          overwritten: 0,
          failed: 0,
          skipped: 0,
          unprocessed: 0,
          historyRecorded: true,
        },
      });
    },
  };
}

function renderJobCenter(client: AnalysisClient, initiallyOpen: boolean) {
  function Harness() {
    const trigger = useRef<HTMLButtonElement>(null);
    const [isOpen, setOpen] = useState(initiallyOpen);
    const parts = useJobCenterParts({
      client,
      isOpen,
      trigger,
      renderEntry: (count, onPress) => (
        <button ref={trigger} onClick={onPress}>
          Tasks {count}
        </button>
      ),
      onOpenChange: setOpen,
      onOpenHistory: () => {},
      onOpenJob: () => {},
    });
    return (
      <>
        {parts.entry}
        {parts.overlay}
      </>
    );
  }
  return render(<Harness />);
}

test("announces a finished scan in the open panel without showing a toast", async () => {
  const fixture = clientFor();
  renderJobCenter(fixture.client, true);
  await screen.findByText("Disk analysis");

  act(() => fixture.finish());

  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toBe(
      "Disk analysis finished",
    ),
  );
  expect(document.querySelector(".du-toast")).toBeNull();
});

test("shows one toast when a scan finishes while the panel is closed", async () => {
  const fixture = clientFor();
  renderJobCenter(fixture.client, false);
  await screen.findByText("Tasks 1");

  act(() => fixture.finish());
  await waitFor(() =>
    expect(document.querySelector(".du-toast")).not.toBeNull(),
  );

  expect(document.querySelectorAll(".du-toast")).toHaveLength(1);
  expect(document.querySelector(".du-toast")?.textContent).toContain(
    "Disk analysis finished",
  );
});
