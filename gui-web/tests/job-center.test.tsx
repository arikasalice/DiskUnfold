import { useRef, useState } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { App } from "../src/app/App";
import { JobCenterPresenter } from "../src/features/jobs/JobCenterPresenter";
import type { AnalysisClient, AnalysisJobListRow } from "../src/ipc";
import type { JobCenterSnapshot } from "../src/state/job-center";

afterEach(cleanup);

const row: AnalysisJobListRow = {
  jobRef: "019a0000-0000-7000-8000-000000000701",
  kind: "scan.metadata",
  scopeSummary: "metadata scan · 1 root",
  scopeFirstRoot: "C:\\Data\\Pictures",
  scopeRootCount: 1,
  state: "running",
  waitReason: "none",
  priority: "normal",
  supportedControls: ["pause", "cancel"],
  sequence: 4,
};

const snapshot: JobCenterSnapshot = {
  cards: [
    {
      row,
      sequence: 4,
      terminalReceived: false,
      order: 0,
    },
  ],
  inProgress: [
    {
      row,
      sequence: 4,
      terminalReceived: false,
      order: 0,
    },
  ],
  recentlyFinished: [],
  pending: new Map(),
  feedback: new Map(),
  loading: false,
  resyncing: false,
  error: false,
};

function renderPanel(
  initialCancelJobRef?: string,
  panelSnapshot: JobCenterSnapshot = snapshot,
) {
  const onControl = vi.fn();
  function Harness() {
    const trigger = useRef<HTMLButtonElement>(null);
    const [isOpen, setOpen] = useState(true);
    return (
      <>
        <button ref={trigger} onClick={() => setOpen(true)}>
          Tasks
        </button>
        <JobCenterPresenter
          isOpen={isOpen}
          trigger={trigger}
          snapshot={panelSnapshot}
          onClose={() => setOpen(false)}
          onOpenHistory={() => setOpen(false)}
          onOpenJob={() => setOpen(false)}
          onRetry={() => {}}
          onControl={onControl}
          {...(initialCancelJobRef ? { initialCancelJobRef } : {})}
        />
      </>
    );
  }
  return { ...render(<Harness />), onControl };
}

test("focuses the title when opened and returns focus to Tasks after Escape", async () => {
  renderPanel();
  const title = screen.getByRole("heading", { name: "Tasks" });
  await waitFor(() => expect(document.activeElement).toBe(title));

  act(() => fireEvent.keyDown(window, { key: "Escape" }));
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Tasks" }),
    ),
  );
  expect(screen.queryByRole("heading", { name: "Tasks" })).toBeNull();
});

test("outside presses leave the panel open and its close button restores focus", async () => {
  renderPanel();
  fireEvent.mouseDown(document.body);
  expect(screen.getByRole("heading", { name: "Tasks" })).toBeDefined();

  fireEvent.click(screen.getByRole("button", { name: "Close tasks" }));
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Tasks" }),
    ),
  );
});

test("renders scope roots through the shared path text handling", () => {
  const scopeFirstRoot = "C:\\Data\\\u202Esecret\\file\u200B.txt";
  const scopedCard = {
    ...snapshot.cards[0]!,
    row: { ...row, scopeFirstRoot },
  };
  const scopedSnapshot = {
    ...snapshot,
    cards: [scopedCard],
    inProgress: [scopedCard],
  };
  const { container } = renderPanel(undefined, scopedSnapshot);
  const target = container.querySelector(".du-job-center__target")!;
  const path = target.querySelector(".du-middle")!;

  expect(path.getAttribute("dir")).toBe("auto");
  expect(path.textContent).toBe(scopeFirstRoot);
  expect(target.querySelector(".du-visually-hidden")?.textContent).toBe(
    scopeFirstRoot,
  );
  expect(target.hasAttribute("title")).toBe(false);
});

test("cancel confirmation starts on the safe Continue action", async () => {
  const { onControl } = renderPanel(row.jobRef);
  const dialog = await screen.findByRole("alertdialog");
  const safeAction = screen.getByRole("button", { name: "Continue" });
  await waitFor(() => expect(document.activeElement).toBe(safeAction));

  fireEvent.click(screen.getByRole("button", { name: "Cancel task" }));
  expect(onControl).toHaveBeenCalledWith(row.jobRef, "cancel");
  expect(dialog.isConnected).toBe(false);
});

test("the Tasks header announces its live active count", async () => {
  const client = {
    call: vi.fn(async (name: string) => {
      if (name === "job.list") return [row];
      throw new Error(`Unexpected command ${name}`);
    }),
    subscribe: () => () => {},
  } as unknown as AnalysisClient;
  render(<App analysis={client} />);

  const tasks = await screen.findByRole("button", {
    name: "Tasks, 1 in progress",
  });
  expect(tasks.querySelector(".du-app-header__count")?.textContent).toBe("1");
});
