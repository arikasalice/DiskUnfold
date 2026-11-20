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
import { HistoryItems } from "../src/features/history/HistoryItems";
import { AnalysisError } from "../src/ipc";
import type { AnalysisClient, HistoryItemsResponse } from "../src/ipc";
afterEach(cleanup);
const page = (
  name: string,
  cursor: string | null = null,
  itemSeq = "0",
): HistoryItemsResponse => ({
  revision: { deleteGeneration: "1", maxRecordSeq: "1" },
  rows: [
    {
      itemSeq,
      result: {
        locationDisplay: `Historical ${name}`,
        outcome: "overwritten",
        reasonKey: null,
        reviewNeeded: true,
        held: false,
        outcomeUnknown: false,
        observedSize: null,
        observedSizeState: "unknown",
        displaced: {
          name: null,
          originalLocationDisplay: "Old location",
          trashedAtMs: null,
          ambiguous: true,
        },
      },
    },
  ],
  counts: { summary: null, recorded: "1", unavailable: null },
  completeness: "complete",
  missingReasons: [],
  nextCursor: cursor,
});
const client = (call: ReturnType<typeof vi.fn>) =>
  ({ call, subscribe: () => () => {} }) as unknown as AnalysisClient;
test("shows separate counts, recorded locations, and display-only displaced details", async () => {
  const call = vi.fn().mockResolvedValue(page("one"));
  render(<HistoryItems client={client(call)} jobRef="job" />);
  // The cell draws the location and reads it from a hidden copy.
  expect(await screen.findAllByText("Historical one")).not.toHaveLength(0);
  expect(
    screen.getByRole("columnheader", { name: "Recorded location" }),
  ).toBeDefined();
  const counts = screen.getByLabelText("Item counts");
  expect(within(counts).getAllByText("Unavailable")).toHaveLength(2);
  expect(within(counts).getByText("1")).toBeDefined();
  const open = screen.getByRole("button", {
    name: "View displaced item details, item 0",
  });
  fireEvent.click(open);
  expect(screen.getByText("Old location")).toBeDefined();
  expect(screen.getByText(/information is ambiguous/)).toBeDefined();
  expect(
    screen.getByText(/Check the operating system trash directly/),
  ).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Close details" }));
  expect(document.activeElement).toBe(open);
});
test("supports every outcome filter and ignores an earlier response", async () => {
  let resolveOld: (value: HistoryItemsResponse) => void = () => {};
  const call = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveOld = resolve;
        }),
    )
    .mockImplementation(() => Promise.resolve(page("new")));
  render(<HistoryItems client={client(call)} jobRef="job" />);
  const select = screen.getByLabelText("Outcome");
  for (const outcome of [
    "completed",
    "overwritten",
    "skipped",
    "failed",
    "unprocessed",
    "all",
  ]) {
    fireEvent.change(select, { target: { value: outcome } });
    await waitFor(() =>
      expect(call).toHaveBeenLastCalledWith("history.items", {
        jobRef: "job",
        outcome: outcome === "all" ? null : outcome,
        cursor: null,
        count: null,
      }),
    );
  }
  await screen.findByText("Historical new");
  await act(async () => resolveOld(page("old")));
  expect(screen.queryByText("Historical old")).toBeNull();
});
test("restarts from page one after a stale cursor", async () => {
  const call = vi
    .fn()
    .mockResolvedValueOnce(page("old", "next"))
    .mockRejectedValueOnce(
      new AnalysisError("STALE_VERSION", "history.version_changed"),
    )
    .mockResolvedValueOnce(page("fresh"));
  render(<HistoryItems client={client(call)} jobRef="job" />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Load more items" }),
  );
  expect(await screen.findByText("Historical fresh")).toBeDefined();
  expect(screen.queryByText("Historical old")).toBeNull();
  expect(screen.getByText(/restarted from the first page/)).toBeDefined();
  expect(call.mock.calls[1]?.[1].cursor).toBe("next");
  expect(call.mock.calls[2]?.[1].cursor).toBeNull();
});
test("ignores results for a previous job", async () => {
  let resolveOld: (value: HistoryItemsResponse) => void = () => {};
  const call = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveOld = resolve;
        }),
    )
    .mockResolvedValue(page("new"));
  const api = client(call);
  const view = render(<HistoryItems client={api} jobRef="old" />);
  view.rerender(<HistoryItems client={api} jobRef="new" />);
  await screen.findByText("Historical new");
  await act(async () => resolveOld(page("old")));
  expect(screen.queryByText("Historical old")).toBeNull();
});

test("retains missing information across pages and focuses displaced details", async () => {
  const first = page("damaged", "next");
  first.rows[0] = { itemSeq: "4", result: null };
  first.completeness = "partial";
  first.missingReasons = ["read_failed"];
  const call = vi
    .fn()
    .mockResolvedValueOnce(first)
    .mockResolvedValueOnce(page("second", null, "9"));
  render(<HistoryItems client={client(call)} jobRef="job" />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Load more items" }),
  );
  await screen.findAllByText("Historical second");
  expect(screen.getByText(/Some records could not be read/)).toBeDefined();
  expect(
    within(screen.getByRole("table", { name: "Item results" })).getByText(
      "Overwritten",
    ),
  ).toBeDefined();
  fireEvent.click(
    screen.getByRole("button", {
      name: "View displaced item details, item 9",
    }),
  );
  expect(document.activeElement).toBe(
    screen.getByRole("region", { name: "Displaced item details" }),
  );
});

test.each([
  ["보고서 ", 1, "보고서 , The name ends with 1 space"],
  ["보고서", 0, "보고서"],
])(
  "F02: the displaced item name %j marks its edge spaces",
  async (name, count, heard) => {
    const withName = page("named");
    withName.rows[0]!.result!.displaced!.name = name;
    render(
      <HistoryItems
        client={client(vi.fn().mockResolvedValue(withName))}
        jobRef="job"
      />,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "View displaced item details, item 0",
      }),
    );
    const value = screen.getByText("Name").closest("div")!.querySelector("dd")!;
    expect(value.querySelectorAll(".du-name__mark")).toHaveLength(count);
    const real = value.cloneNode(true) as Element;
    real.querySelectorAll(".du-name__mark").forEach((mark) => mark.remove());
    expect(real.textContent).toBe(heard);
  },
);

test("distinguishes an empty filter from an unavailable summary", async () => {
  const empty = { ...page("unused"), rows: [] };
  const call = vi.fn().mockResolvedValue(empty);
  render(<HistoryItems client={client(call)} jobRef="job" />);
  await screen.findByText(/summary count is unavailable/);
  fireEvent.change(screen.getByLabelText("Outcome"), {
    target: { value: "failed" },
  });
  await screen.findByText("No recorded items match this outcome.");
});

test("opens a full recorded item with a long location, lower-bound size and every flag", async () => {
  const full = page("long");
  const result = full.rows[0]?.result;
  if (!result) throw new Error("Missing item fixture");
  result.locationDisplay =
    "D:\\" + "long-folder\\".repeat(30) + "recorded-file.txt";
  result.displaced = null;
  result.observedSize = "9223372036854775807";
  result.observedSizeState = "lower_bound";
  result.held = true;
  result.outcomeUnknown = true;
  result.reasonKey = "private.reason-key";
  const call = vi.fn().mockResolvedValue(full);
  render(<HistoryItems client={client(call)} jobRef="job" />);
  const open = await screen.findByRole("button", {
    name: "View item result, item 0",
  });
  open.focus();
  fireEvent.keyDown(open, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(open, { key: "Enter", code: "Enter" });
  const details = screen.getByRole("region", { name: "Recorded item details" });
  expect(document.activeElement).toBe(details);
  expect(within(details).getByText(result.locationDisplay)).toBeDefined();
  expect(
    within(details).getByText("9223372036854775807 bytes or more"),
  ).toBeDefined();
  expect(
    within(details).getByText(
      "Review needed · Held · Outcome unknown · A reason was recorded",
    ),
  ).toBeDefined();
  expect(screen.queryByText("private.reason-key")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Close details" }));
  expect(document.activeElement).toBe(open);
});

// #170, F02 README: the location keeps the end of the path, takes no tab stop
// even when cut, has no browser title, and marks the edge spaces of its name.
test("the recorded location keeps its end, takes no tab stop and marks edge spaces", async () => {
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get: () => 40,
  });
  Object.defineProperty(HTMLElement.prototype, "scrollWidth", {
    configurable: true,
    get: () => 400,
  });
  try {
    const spaced = page("spaced");
    const location =
      "C:\\Users\\someone\\Documents\\Projects\\2026\\quarterly report.xlsx ";
    spaced.rows[0]!.result!.locationDisplay = location;
    const { container } = render(
      <HistoryItems
        client={client(vi.fn().mockResolvedValue(spaced))}
        jobRef="job"
      />,
    );
    await screen.findByRole("button", {
      name: "View displaced item details, item 0",
    });
    const cell = container.querySelector(".du-history-item-location")!;
    expect(cell.querySelector("[title]")).toBeNull();
    expect(cell.hasAttribute("title")).toBe(false);
    const frame = cell.querySelector(".du-fit")!;
    expect(frame.hasAttribute("tabindex")).toBe(false);
    expect(frame.hasAttribute("role")).toBe(false);
    // The end of the path stays whole, with the extension and the mark.
    const end = cell.querySelector(".du-middle__end")!;
    expect(end.textContent).toContain(".xlsx");
    expect(cell.querySelectorAll(".du-middle .du-name__mark")).toHaveLength(1);
    // Screen readers hear the full path and that the name ends with a space.
    const heard = cell.querySelector(":scope .du-fit > .du-visually-hidden")!;
    const copy = heard.cloneNode(true) as Element;
    copy
      .querySelectorAll('[aria-hidden="true"]')
      .forEach((node) => node.remove());
    expect(copy.textContent).toBe(location + ", The name ends with 1 space");
    // The pointer's full text draws the mark too.
    expect(cell.querySelector(".du-fit__full .du-name__mark")).not.toBeNull();
  } finally {
    delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
    delete (HTMLElement.prototype as { scrollWidth?: number }).scrollWidth;
  }
});
