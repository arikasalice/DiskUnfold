import { useRef, useState } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
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
    row: { ...row, scopeFirstRoot, scopeRootCount: 2 },
  };
  const scopedSnapshot = {
    ...snapshot,
    cards: [scopedCard],
    inProgress: [scopedCard],
  };
  const { container } = renderPanel(undefined, scopedSnapshot);
  const target = container.querySelector(".du-scope-target")!;
  const path = target.querySelector(".du-middle")!;
  const pathFrame = path.closest(".du-fit--cell")!;

  expect(path.getAttribute("dir")).toBe("auto");
  expect(path.textContent).toBe(scopeFirstRoot);
  expect(pathFrame.querySelector(".du-visually-hidden")?.textContent).toBe(
    scopeFirstRoot,
  );
  expect(pathFrame.classList.contains("du-fit--cell")).toBe(true);
  expect(target.querySelector(".du-text")?.textContent).toBe("and 1 more");
  expect(pathFrame.hasAttribute("tabindex")).toBe(false);
  expect(target.hasAttribute("title")).toBe(false);
});

test("every in-progress card keeps View on its bottom line and is not a button", () => {
  const active = (["running", "queued", "paused"] as const).map(
    (state, index) => {
      const supportedControls: AnalysisJobListRow["supportedControls"] =
        state === "running"
          ? ["pause", "cancel"]
          : state === "queued"
            ? ["cancel"]
            : ["resume", "cancel"];
      const waitReason: AnalysisJobListRow["waitReason"] =
        state === "queued" ? "queue_full" : "none";
      return {
        ...snapshot.cards[0]!,
        row: {
          ...row,
          jobRef: `${row.jobRef}-${state}`,
          state,
          waitReason,
          supportedControls,
        },
        sequence: index,
        order: index,
      };
    },
  );
  const pending = new Map(snapshot.pending);
  pending.set(active[0]!.row.jobRef, "pause");
  const activeSnapshot: JobCenterSnapshot = {
    ...snapshot,
    cards: active,
    inProgress: active,
    pending,
  };
  const { container } = renderPanel(undefined, activeSnapshot);
  const cards = container.querySelectorAll<HTMLElement>(
    ".du-job-center__cards .du-job-center__card",
  );

  expect(cards).toHaveLength(active.length);
  for (const card of cards) {
    expect(card.tagName).toBe("ARTICLE");
    expect(card.matches("button, [role='button']")).toBe(false);
    const view = within(card).getByRole("button", { name: "View" });
    const line = view.parentElement;
    expect(line?.classList.contains("du-job-center__state-line")).toBe(true);
  }

  const queuedBadge = cards[1]!.querySelector(".du-badge")!;
  expect(queuedBadge.classList.contains("du-badge--neutral")).toBe(true);
  expect(queuedBadge.getAttribute("data-status-shape")).toBe("clock");
  expect(
    queuedBadge.querySelector("svg")?.classList.contains("lucide-clock"),
  ).toBe(true);
  expect(
    cards[0]!.querySelector(".du-job-center__state-line")?.textContent,
  ).toContain("Pausing…");
});

test("cancel confirmation starts on the safe Continue action", async () => {
  const { onControl } = renderPanel(row.jobRef);
  const dialog = await screen.findByRole("alertdialog");
  const safeAction = screen.getByRole("button", { name: "Continue" });
  const cancelAction = screen.getByRole("button", { name: "Cancel task" });
  await waitFor(() => expect(document.activeElement).toBe(safeAction));
  expect(safeAction.classList.contains("du-action--primary")).toBe(true);
  expect(cancelAction.classList.contains("du-action--secondary")).toBe(true);
  expect(dialog.querySelector(".du-inspection-dialog__body")).toBeNull();

  fireEvent.click(cancelAction);
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
