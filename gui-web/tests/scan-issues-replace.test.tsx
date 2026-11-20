import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { Profiler } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { ScanIssues } from "../src/features/analysis/ScanIssues";
import { AnalysisError } from "../src/ipc";
import type {
  AnalysisClient,
  ScanFailures,
  ScanFailuresRequest,
} from "../src/ipc";

/*
 * F02 FR3.HD1.gui-web.DD1 (design review rule 10, PR #69 review): the scan
 * issue list shows rows of one result. A newer result replaces the loaded
 * rows at once; "Load more" never appends rows of one result to rows of
 * another, and a read that became obsolete stops asking for pages.
 */

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

type Reply = ScanFailures | Promise<ScanFailures>;

/** One row per page; the row and the cursor name the result they belong to. */
function page(version: string, index: number, total: number): ScanFailures {
  return {
    resultVersion: version,
    totalCount: total,
    rows: [
      {
        kind: "unread_range",
        stage: "enumerate",
        pathRef: { rootIndex: 0, relative: [] },
        reasonKey: "scan.path_denied",
        displayPath: version + "-" + index,
      },
    ],
    ...(index + 1 < total && { nextCursor: version + "@" + (index + 1) }),
  };
}

/** The page index a cursor asks for; like the engine's, it names no result. */
function indexOf(cursor: string | null | undefined) {
  return cursor ? Number(cursor.split("@")[1]) : 0;
}

/**
 * A list of `total` issues per result. `reply` may answer a request itself;
 * otherwise the page the request names is returned at once.
 */
function issues(
  total: number,
  reply: (request: ScanFailuresRequest, index: number) => Reply | undefined,
) {
  const requests: ScanFailuresRequest[] = [];
  const reopen = vi.fn();
  const client = {
    call: (async (name: string, body: ScanFailuresRequest) => {
      if (name !== "scan.failures") throw new Error("Unexpected " + name);
      requests.push(body);
      const index = indexOf(body.cursor);
      return reply(body, index) ?? page(body.resultVersion, index, total);
    }) as AnalysisClient["call"],
    subscribe: () => () => {},
  } satisfies AnalysisClient;
  /** The results whose rows each React commit showed. */
  const commits: string[][] = [];
  const onRender = () => {
    const versions = screen
      .queryAllByText(/^v\d+-\d+$/)
      .map((cell) => cell.textContent.split("-")[0]!);
    commits.push([...new Set(versions)]);
  };
  const element = (version: string) => (
    <Profiler id="issues" onRender={onRender}>
      <ScanIssues
        client={client}
        resultVersion={version}
        onReopenLatest={reopen}
      />
    </Profiler>
  );
  const view = render(element("v1"));
  fireEvent.click(screen.getByRole("button", { name: "View scan issues" }));
  return {
    requests,
    reopen,
    show: (version: string) => view.rerender(element(version)),
    unmount: view.unmount,
    /** Requests for `version`, as the cursors they carried. */
    asked: (version: string) =>
      requests
        .filter(({ resultVersion }) => resultVersion === version)
        .map(({ cursor }) => cursor ?? null),
    /** Commits that showed rows of more than one result. */
    mixed: () => commits.filter((versions) => versions.length > 1),
  };
}

function deferred() {
  let resolve: (value: ScanFailures) => void = () => {};
  let reject: (reason?: unknown) => void = () => {};
  const promise = new Promise<ScanFailures>((settle, fail) => {
    resolve = settle;
    reject = fail;
  });
  return { promise, resolve, reject };
}

const stale = () =>
  new AnalysisError("STALE_VERSION", "result.requery_required");
const loadMore = () =>
  screen.getByRole<HTMLButtonElement>("button", { name: "Load more issues" });
const shown = () =>
  screen.queryAllByText(/^v\d+-\d+$/).map((cell) => cell.textContent);
const reading = () =>
  screen.queryByRole("progressbar", { name: "Reading scan issues" });

/** Loads `count` rows of the first result, one page at a time. */
async function loadRows(count: number) {
  await screen.findByText("v1-0");
  for (let index = 1; index < count; index += 1) {
    fireEvent.click(loadMore());
    await screen.findByText("v1-" + index);
  }
}

test("F02 FR4.HD1.app-host.DD2: a stale replacement offers recovery without retrying automatically", async () => {
  const list = issues(4, ({ resultVersion }) =>
    resultVersion === "v2" ? Promise.reject(stale()) : undefined,
  );
  await loadRows(2);
  list.show("v2");
  const reopen = await screen.findByRole<HTMLButtonElement>("button", {
    name: "Reopen with the new result",
  });
  await act(async () => {});
  expect(shown()).toEqual(["v1-0", "v1-1"]);
  expect(reading()).toBeNull();
  expect(list.asked("v2")).toEqual([null]);
  expect(list.reopen).not.toHaveBeenCalled();

  fireEvent.click(reopen);
  expect(list.reopen).toHaveBeenCalledTimes(1);
  expect(list.asked("v2")).toEqual([null]);
  list.show("v3");
  await screen.findByText("v3-1");
  expect(shown()).toEqual(["v3-0", "v3-1"]);
  fireEvent.click(loadMore());
  await screen.findByText("v3-2");
  expect(list.asked("v3")).toEqual([null, "v3@1", "v3@2"]);
  expect(shown()).toEqual(["v3-0", "v3-1", "v3-2"]);
  expect(list.mixed()).toEqual([]);
  expect(screen.getByText("Showing 3 of 4 issues")).toBeDefined();
});

test("F02 FR4.HD1.app-host.DD2: Load more owed during a stale replacement continues only after reopening latest", async () => {
  const replacement = deferred();
  const list = issues(4, ({ resultVersion }) =>
    resultVersion === "v2" ? replacement.promise : undefined,
  );
  await loadRows(2);
  list.show("v2");
  await waitFor(() => expect(list.asked("v2")).toEqual([null]));

  fireEvent.click(loadMore());
  expect(list.asked("v2")).toEqual([null]);
  await act(async () => replacement.reject(stale()));
  const reopen = await screen.findByRole<HTMLButtonElement>("button", {
    name: "Reopen with the new result",
  });
  expect(shown()).toEqual(["v1-0", "v1-1"]);
  expect(reading()).toBeNull();
  expect(list.asked("v2")).toEqual([null]);
  expect(list.reopen).not.toHaveBeenCalled();

  fireEvent.click(reopen);
  expect(list.reopen).toHaveBeenCalledTimes(1);
  expect(list.asked("v2")).toEqual([null]);
  list.show("v3");
  await screen.findByText("v3-2");
  expect(shown()).toEqual(["v3-0", "v3-1", "v3-2"]);
  expect(list.asked("v3")).toEqual([null, "v3@1", "v3@2"]);
  expect(list.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: Load more during a replacement reads on after the rows are replaced", async () => {
  const first = deferred();
  const list = issues(4, ({ resultVersion }, index) =>
    resultVersion === "v2" && index === 0 ? first.promise : undefined,
  );
  await loadRows(2);
  list.show("v2");
  await waitFor(() => expect(list.asked("v2")).toEqual([null]));
  fireEvent.click(loadMore());
  fireEvent.click(loadMore());
  expect(list.asked("v2")).toEqual([null]);
  expect(shown()).toEqual(["v1-0", "v1-1"]);

  await act(async () => first.resolve(page("v2", 0, 4)));
  await screen.findByText("v2-2");
  expect(shown()).toEqual(["v2-0", "v2-1", "v2-2"]);
  expect(list.asked("v2")).toEqual([null, "v2@1", "v2@2"]);
  expect(list.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: a replacing read stops asking for pages once a newer result replaces it, and its late page is ignored", async () => {
  const first = deferred();
  const list = issues(5, ({ resultVersion }, index) =>
    resultVersion === "v2" && index === 0 ? first.promise : undefined,
  );
  await loadRows(3);
  list.show("v2");
  await waitFor(() => expect(list.asked("v2")).toEqual([null]));
  list.show("v3");
  await screen.findByText("v3-2");
  expect(shown()).toEqual(["v3-0", "v3-1", "v3-2"]);

  // The first page of v2 arrives late; nothing more is asked for v2.
  await act(async () => first.resolve(page("v2", 0, 5)));
  expect(list.asked("v2")).toEqual([null]);
  expect(list.asked("v3")).toEqual([null, "v3@1", "v3@2"]);
  expect(shown()).toEqual(["v3-0", "v3-1", "v3-2"]);
  expect(list.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: a filter change stops the pages of the read it replaces", async () => {
  const first = deferred();
  const list = issues(5, ({ resultVersion, kind }, index) =>
    resultVersion === "v2" && !kind && index === 0 ? first.promise : undefined,
  );
  await loadRows(3);
  list.show("v2");
  await waitFor(() => expect(list.asked("v2")).toEqual([null]));
  fireEvent.change(screen.getByRole("combobox", { name: "Issue type" }), {
    target: { value: "unread_range" },
  });
  await screen.findByText("v2-0");
  await act(async () => first.resolve(page("v2", 0, 5)));
  expect(list.requests.filter(({ kind }) => !kind).slice(3)).toEqual([
    { resultVersion: "v2" },
  ]);
  expect(shown()).toEqual(["v2-0"]);
});

test("F02 FR4.HD1.app-host.DD2: a late page after a stale replacement is ignored until the user reopens latest", async () => {
  const more = deferred();
  let refuse = true;
  const list = issues(4, ({ resultVersion }, index) => {
    if (resultVersion === "v1" && index === 1) return more.promise;
    if (resultVersion !== "v2" || !refuse) return undefined;
    refuse = false;
    return Promise.reject(stale());
  });
  await screen.findByText("v1-0");
  fireEvent.click(loadMore());
  expect(reading()).not.toBeNull();
  // The result changes during the read; its replacement is refused in turn.
  list.show("v2");
  await waitFor(() => expect(list.asked("v2")).toEqual([null]));
  const reopen = await screen.findByRole<HTMLButtonElement>("button", {
    name: "Reopen with the new result",
  });
  await act(async () => more.resolve(page("v1", 1, 4)));
  expect(shown()).toEqual(["v1-0"]);
  expect(reading()).toBeNull();
  expect(list.asked("v2")).toEqual([null]);
  expect(list.reopen).not.toHaveBeenCalled();

  fireEvent.click(reopen);
  expect(list.reopen).toHaveBeenCalledTimes(1);
  list.show("v3");
  await screen.findByText("v3-1");
  expect(shown()).toEqual(["v3-0", "v3-1"]);
  expect(list.asked("v3")).toEqual([null, "v3@1"]);
  expect(list.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: a replacement that fails during a Load more read leaves Retry usable", async () => {
  const more = deferred();
  let fail = true;
  const list = issues(4, ({ resultVersion }, index) => {
    if (resultVersion === "v1" && index === 1) return more.promise;
    if (resultVersion !== "v2" || !fail) return undefined;
    fail = false;
    return Promise.reject(new Error("socket closed"));
  });
  await screen.findByText("v1-0");
  fireEvent.click(loadMore());
  list.show("v2");
  const retry = await screen.findByRole<HTMLButtonElement>("button", {
    name: "Retry scan issues",
  });
  expect(retry.disabled).toBe(false);
  expect(reading()).toBeNull();
  expect(shown()).toEqual(["v1-0"]);

  fireEvent.click(retry);
  await screen.findByText("v2-0");
  expect(shown()).toEqual(["v2-0"]);
  expect(list.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: unmounting during a read asks for no further pages and updates nothing", async () => {
  const first = deferred();
  const list = issues(5, ({ resultVersion }, index) =>
    resultVersion === "v2" && index === 0 ? first.promise : undefined,
  );
  await loadRows(3);
  list.show("v2");
  await waitFor(() => expect(list.asked("v2")).toEqual([null]));
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  list.unmount();
  const asked = list.requests.length;
  await act(async () => first.resolve(page("v2", 0, 5)));
  await act(async () => {});
  expect(list.requests).toHaveLength(asked);
  expect(errors).not.toHaveBeenCalled();
});
