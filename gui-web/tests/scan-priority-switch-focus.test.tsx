import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import type { AnalysisClient, BrowseOpenResponse } from "../src/ipc";
import { useScanPriority } from "../src/state/scan-priority";
import type { ScanPriorityContext } from "../src/state/scan-priority";
import {
  gridRow,
  newer,
  openedRoot,
  priorityClient,
  running,
  version,
} from "./scan-priority-fixture";

/*
 * F02 FR8.HD1, PR #60 design review: "Show latest result" keeps focus in the
 * selection action bar while the list switches to the scan's newest result,
 * and never leaves it on the panel, which would draw a frame around it.
 */

afterEach(cleanup);

const newest = "018f0000-0000-7000-8000-0000000000a3";
const REASON =
  "A newer result is available · Switch to it to analyze a folder first";

/** A list that follows the scan's result it was switched to, as the app's. */
function Harness({
  client,
  context,
  browsePort,
}: {
  client: AnalysisClient;
  context: ScanPriorityContext | undefined;
  browsePort: BrowsePort;
}) {
  const [shown, setShown] = useState(version);
  const priority = useScanPriority(client, context, {
    jobRef: "scan",
    resultVersion: shown,
    open: setShown,
  });
  return (
    <BrowsePanel
      port={browsePort}
      resultVersion={shown}
      priority={priority}
      inspectItem={() => "Details"}
    />
  );
}

/** Opens the first result at once and holds every later one until released or failed. */
function heldPort() {
  const held: { resolve: () => void; reject: () => void }[] = [];
  const browsePort: BrowsePort = {
    open: vi.fn((request) => {
      const response = openedRoot(
        request.resultVersion ?? version,
        request.requestSeq,
      );
      if (request.resultVersion === version) return Promise.resolve(response);
      return new Promise<BrowseOpenResponse>((resolve, reject) =>
        held.push({
          resolve: () => resolve(response),
          reject: () => reject(new Error("unavailable")),
        }),
      );
    }),
    page: vi.fn(),
    copyPath: vi.fn(),
  };
  return { browsePort, held };
}

const moved: ScanPriorityContext = { ...running, resultVersion: newer };

async function setup(context: ScanPriorityContext = moved) {
  const { client } = priorityClient();
  const { browsePort, held } = heldPort();
  const rendered = render(
    <Harness client={client} context={context} browsePort={browsePort} />,
  );
  await screen.findByText("Photos");
  const rerender = (next: ScanPriorityContext | undefined) =>
    rendered.rerender(
      <Harness client={client} context={next} browsePort={browsePort} />,
    );
  const release = async () => {
    await act(async () => held.shift()?.resolve());
  };
  const fail = async () => {
    await act(async () => held.shift()?.reject());
  };
  return { browsePort, rerender, release, fail };
}

const bar = () => screen.queryByRole("toolbar", { name: "Selection actions" });
const barAction = (name: string) =>
  within(bar()!).queryByRole("button", { name });
const panel = () => screen.getByRole("region", { name: "Folder navigation" });

function pressByKeyboard(button: HTMLElement) {
  act(() => button.focus());
  fireEvent.keyDown(button, { key: "Enter" });
  fireEvent.keyUp(button, { key: "Enter" });
}

test("F02 FR8.HD1: the bar stays with both actions unavailable while the list switches", async () => {
  const { browsePort } = await setup();
  const showLatest = barAction("Show latest result")!;
  pressByKeyboard(showLatest);
  expect(vi.mocked(browsePort.open)).toHaveBeenLastCalledWith(
    expect.objectContaining({ resultVersion: newer }),
    expect.anything(),
  );
  // The earlier result is still shown and the bar keeps the focus.
  expect(bar()).not.toBeNull();
  expect(screen.getByText(REASON)).toBeDefined();
  expect(document.activeElement).toBe(showLatest);
  expect(showLatest.getAttribute("aria-disabled")).toBe("true");
  expect(barAction("Analyze first")!.getAttribute("aria-disabled")).toBe(
    "true",
  );
  // Pressing it again while the list switches opens nothing more.
  const opens = vi.mocked(browsePort.open).mock.calls.length;
  pressByKeyboard(showLatest);
  expect(vi.mocked(browsePort.open).mock.calls.length).toBe(opens);
});

test("F02 FR8.HD1: focus moves to Analyze first once the newest result opens without Show latest result", async () => {
  const { release } = await setup();
  pressByKeyboard(barAction("Show latest result")!);
  await release();
  expect(screen.queryByText(REASON)).toBeNull();
  expect(barAction("Show latest result")).toBeNull();
  const analyzeFirst = barAction("Analyze first")!;
  expect(document.activeElement).toBe(analyzeFirst);
  expect(document.activeElement).not.toBe(panel());
});

test("F02 FR8.HD1: focus moves to the list when the newest result shows no bar", async () => {
  const { rerender, release } = await setup();
  pressByKeyboard(barAction("Show latest result")!);
  // The scan ends while the list switches.
  rerender(undefined);
  expect(document.activeElement).toBe(barAction("Show latest result"));
  await release();
  expect(bar()).toBeNull();
  expect(document.activeElement).toBe(gridRow("Photos"));
});

test("F02 FR8.HD1: focus stays on Show latest result when the opened result is already behind again", async () => {
  const { rerender, release } = await setup();
  pressByKeyboard(barAction("Show latest result")!);
  // The running scan publishes its next result while the list switches.
  rerender({ ...running, resultVersion: newest });
  await release();
  const showLatest = barAction("Show latest result")!;
  expect(showLatest.getAttribute("aria-disabled")).toBeNull();
  expect(document.activeElement).toBe(showLatest);
  expect(document.activeElement).not.toBe(barAction("Analyze first"));
});

test("F02 FR8.HD1: a failed switch makes Show latest result available again", async () => {
  const { browsePort, fail, release } = await setup();
  const showLatest = barAction("Show latest result")!;
  pressByKeyboard(showLatest);
  await fail();
  // The existing failure notice reports it; the bar stays on the earlier list.
  expect(screen.getByRole("alert").textContent).toContain("Could not open");
  expect(screen.getByText(REASON)).toBeDefined();
  const again = barAction("Show latest result")!;
  expect(again.getAttribute("aria-disabled")).toBeNull();
  expect(document.activeElement).toBe(again);
  expect(barAction("Analyze first")!.getAttribute("aria-disabled")).toBe(
    "true",
  );
  // Pressing it again opens the newest result again and holds the bar.
  const opens = vi.mocked(browsePort.open).mock.calls.length;
  pressByKeyboard(again);
  expect(vi.mocked(browsePort.open).mock.calls.length).toBe(opens + 1);
  expect(vi.mocked(browsePort.open)).toHaveBeenLastCalledWith(
    expect.objectContaining({ resultVersion: newer }),
    expect.anything(),
  );
  expect(barAction("Show latest result")!.getAttribute("aria-disabled")).toBe(
    "true",
  );
  await release();
  expect(screen.queryByText(REASON)).toBeNull();
  expect(document.activeElement).toBe(barAction("Analyze first"));
});

test("F02 FR8.HD1: a switch answered with results still being prepared releases the bar", async () => {
  const { client } = priorityClient();
  const browsePort: BrowsePort = {
    open: vi.fn((request) =>
      Promise.resolve(
        request.resultVersion === version
          ? openedRoot(version, request.requestSeq)
          : {
              kind: "job" as const,
              requestSeq: request.requestSeq,
              jobRef: "prepare",
              resultVersion: request.resultVersion ?? newer,
            },
      ),
    ),
    page: vi.fn(),
    copyPath: vi.fn(),
  };
  render(<Harness client={client} context={moved} browsePort={browsePort} />);
  await screen.findByText("Photos");
  pressByKeyboard(barAction("Show latest result")!);
  await screen.findByText("Folder results are being prepared.");
  expect(barAction("Show latest result")!.getAttribute("aria-disabled")).toBe(
    null,
  );
});

test("F02 FR8.HD1: focus stays on Analyze first when a newer result arrives again", async () => {
  const { rerender, release } = await setup();
  pressByKeyboard(barAction("Show latest result")!);
  await release();
  const analyzeFirst = barAction("Analyze first")!;
  expect(document.activeElement).toBe(analyzeFirst);
  // A running scan publishes its next result shortly after.
  rerender({ ...running, resultVersion: newest });
  expect(screen.getByText(REASON)).toBeDefined();
  expect(barAction("Show latest result")).not.toBeNull();
  expect(barAction("Analyze first")).toBe(analyzeFirst);
  expect(document.activeElement).toBe(analyzeFirst);
});

test("F02 FR8.HD1: the row menu draws the reason inside its prose menu item", async () => {
  await setup();
  fireEvent.contextMenu(gridRow("Photos"));
  const menu = await screen.findByRole("menu", { name: "Actions for Photos" });
  const reason = within(menu).getByText(REASON);
  expect(reason.closest('.du-item-menu [role="menuitem"]')).not.toBeNull();
});
