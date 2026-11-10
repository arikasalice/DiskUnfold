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
  expect(await screen.findByText("Historical one")).toBeDefined();
  const counts = screen.getByLabelText("Item counts");
  expect(within(counts).getAllByText("Unavailable")).toHaveLength(2);
  expect(within(counts).getByText("1")).toBeDefined();
  fireEvent.click(
    screen.getByRole("button", {
      name: "View displaced item details, item 0",
    }),
  );
  expect(screen.getByText("Old location")).toBeDefined();
  expect(screen.getByText(/information is ambiguous/)).toBeDefined();
  expect(
    screen.getByText(/Check the operating system trash directly/),
  ).toBeDefined();
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
  await screen.findByText("Historical second");
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
