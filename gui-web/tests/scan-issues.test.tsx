import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ScanIssues } from "../src/features/analysis/ScanIssues";
import { AnalysisError } from "../src/ipc";
import type {
  AnalysisClient,
  ScanFailureRow,
  ScanFailures,
  ScanFailuresRequest,
} from "../src/ipc";

afterEach(cleanup);

function row(displayPath: string, overrides: Partial<ScanFailureRow> = {}) {
  return {
    kind: "unread_range",
    stage: "enumerate",
    pathRef: { rootIndex: 0, relative: [] },
    reasonKey: "scan.path_denied",
    displayPath,
    ...overrides,
  } satisfies ScanFailureRow;
}

function issuesClient(
  read: (request: ScanFailuresRequest) => Promise<ScanFailures>,
) {
  const requests: ScanFailuresRequest[] = [];
  const client = {
    call: (async (name: string, body: ScanFailuresRequest) => {
      if (name !== "scan.failures") throw new Error("Unexpected " + name);
      requests.push(body);
      return read(body);
    }) as AnalysisClient["call"],
    subscribe: () => () => {},
  } satisfies AnalysisClient;
  return { client, requests };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

function openIssues() {
  fireEvent.click(screen.getByRole("button", { name: "View scan issues" }));
}

test("F02 FR3.HD1.gui-web.DD1: a late page for the previous result version is discarded", async () => {
  const first = deferred<ScanFailures>();
  const { client, requests } = issuesClient(async ({ resultVersion }) =>
    resultVersion === "v1"
      ? first.promise
      : { resultVersion, totalCount: 1, rows: [row("C:\\v2")] },
  );
  const view = render(
    <ScanIssues client={client} resultVersion="v1" onReopenLatest={vi.fn()} />,
  );
  openIssues();
  view.rerender(
    <ScanIssues client={client} resultVersion="v2" onReopenLatest={vi.fn()} />,
  );
  await screen.findByText("C:\\v2");
  await act(async () =>
    first.resolve({
      resultVersion: "v1",
      totalCount: 2,
      rows: [row("C:\\v1")],
      nextCursor: "old",
    }),
  );
  expect(screen.queryByText("C:\\v1")).toBeNull();
  expect(screen.getByText("Showing 1 of 1 issues")).toBeDefined();
  expect(screen.queryByRole("button", { name: "Load more issues" })).toBeNull();
  expect(requests).toEqual([{ resultVersion: "v1" }, { resultVersion: "v2" }]);
});

test("F02 FR3.HD1.gui-web.DD1: changing the filter drops the previous cursor and its late page", async () => {
  const late = deferred<ScanFailures>();
  const { client, requests } = issuesClient(async ({ kind, cursor }) => {
    if (cursor === "all-2") return late.promise;
    if (kind === "untraversed_link")
      return {
        resultVersion: "v1",
        totalCount: 2,
        rows: [row(cursor ?? "link-1", { kind })],
        ...(!cursor && { nextCursor: "link-2" }),
      };
    return {
      resultVersion: "v1",
      totalCount: 2,
      rows: [row("all-1")],
      nextCursor: "all-2",
    };
  });
  render(
    <ScanIssues client={client} resultVersion="v1" onReopenLatest={vi.fn()} />,
  );
  openIssues();
  await screen.findByText("all-1");
  fireEvent.click(screen.getByRole("button", { name: "Load more issues" }));
  fireEvent.change(screen.getByRole("combobox", { name: "Issue type" }), {
    target: { value: "untraversed_link" },
  });
  await screen.findByText("link-1");
  await act(async () =>
    late.resolve({ resultVersion: "v1", totalCount: 2, rows: [row("all-2")] }),
  );
  expect(screen.queryByText("all-2")).toBeNull();
  expect(screen.queryByText("all-1")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Load more issues" }));
  await screen.findByText("link-2");
  expect(requests).toEqual([
    { resultVersion: "v1" },
    { resultVersion: "v1", cursor: "all-2" },
    { resultVersion: "v1", kind: "untraversed_link" },
    { resultVersion: "v1", kind: "untraversed_link", cursor: "link-2" },
  ]);
});

test.each([
  [
    "STALE_VERSION",
    "These issues belong to a result that has been replaced. Reopen them with the new result.",
    "Reopen with the new result",
  ],
  [
    "NOT_FOUND",
    "The result for these issues is no longer available. Reopen the latest result.",
    "Reopen the latest result",
  ],
] as const)(
  "F02 IF-05: %s offers to reopen the latest result without switching automatically",
  async (code, message, action) => {
    const { client, requests } = issuesClient(async () => {
      throw new AnalysisError(code, "result.requery_required");
    });
    const reopen = vi.fn();
    render(
      <ScanIssues client={client} resultVersion="v1" onReopenLatest={reopen} />,
    );
    openIssues();
    await screen.findByText(message);
    expect(screen.queryByText("result.requery_required")).toBeNull();
    expect(reopen).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: action }));
    expect(reopen).toHaveBeenCalledTimes(1);
    await screen.findByText(message);
    expect(requests.every((request) => request.resultVersion === "v1")).toBe(
      true,
    );
  },
);

test("F02 IF-05: other failures use general copy and retry the same page", async () => {
  let attempts = 0;
  const { client, requests } = issuesClient(async ({ resultVersion }) => {
    attempts += 1;
    if (attempts === 1) throw new Error("socket closed");
    return { resultVersion, totalCount: 1, rows: [row("C:\\after")] };
  });
  const reopen = vi.fn();
  render(
    <ScanIssues client={client} resultVersion="v1" onReopenLatest={reopen} />,
  );
  openIssues();
  await screen.findByText("Scan issues could not be read.");
  expect(screen.queryByText("socket closed")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Retry scan issues" }));
  await screen.findByText("C:\\after");
  expect(reopen).not.toHaveBeenCalled();
  expect(requests).toEqual([{ resultVersion: "v1" }, { resultVersion: "v1" }]);
});

test("F02 NFR5.HD1.gui-web.DD1: issues render in a named virtual table with general labels for unknown values", async () => {
  const unknown = {
    kind: "future_kind",
    stage: "future_stage",
    reasonKey: "scan.future_reason",
  } as unknown as Partial<ScanFailureRow>;
  const rows = Array.from({ length: 200 }, (_, index) =>
    row("C:\\item-" + index, index === 0 ? unknown : {}),
  );
  const { client } = issuesClient(async ({ resultVersion }) => ({
    resultVersion,
    totalCount: 400,
    rows,
    nextCursor: "next",
  }));
  render(
    <ScanIssues client={client} resultVersion="v1" onReopenLatest={vi.fn()} />,
  );
  openIssues();
  const table = await screen.findByRole("table", {
    name: "Scan issue entries",
  });
  expect(table.getAttribute("aria-rowcount")).toBe("401");
  expect(within(table).getAllByRole("row").length).toBeLessThan(30);
  const first = within(table).getByRole("row", { name: "Issue 1" });
  expect(within(first).getByText("Unknown issue")).toBeDefined();
  expect(within(first).getByText("Unknown stage")).toBeDefined();
  expect(within(first).getByText("Unknown reason")).toBeDefined();
  expect(
    screen
      .getByRole("region", { name: "Scan issue entries scroll area" })
      .getAttribute("tabindex"),
  ).toBe("0");
  expect(screen.getByText("Showing 200 of 400 issues")).toBeDefined();
});
