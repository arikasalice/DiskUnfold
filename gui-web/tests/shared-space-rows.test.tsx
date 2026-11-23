import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { sharedSpaceRows } from "../src/features/analysis/shared-space-rows";
import { ModelRows } from "../src/features/visualization/ModelRows";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import type { AnalysisClient, SpaceDetail } from "../src/ipc";

afterEach(cleanup);

const text: VizText = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) => message.replace(`{${name}}`, String(value)),
      visualizationMessage("en", key),
    ),
  { system: "iec", unit: "auto" },
);

/** Row 0 is the counted path, row 1 a path that shares its space; row 2 is ordinary. */
function shown(sizeBasis: "allocated" | "logical" | undefined) {
  const input = modelFixture();
  input.frame.model.representation.sizeBasis = "allocated";
  const rows = input.page.response.rows;
  rows[0] = { ...rows[0]!, sharesSpace: true };
  rows[1] = {
    ...rows[1]!,
    sharesSpace: true,
    sharedCountedItem: rows[0]!.itemRef!,
  };
  if (sizeBasis === undefined) delete input.page.response.sizeBasis;
  else input.page.response.sizeBasis = sizeBasis;
  render(
    <ModelRows
      page={input.page}
      model={input.frame.model}
      text={text}
      expanded
    />,
  );
  return Array.from(document.querySelectorAll("tbody tr")).map(
    (row) => row.textContent ?? "",
  );
}

const fallbackText = ((
  _key: string,
  fallback: string,
  values?: Record<string, string>,
) =>
  Object.entries(values ?? {}).reduce(
    (message, [name, value]) => message.replace(`{${name}}`, value),
    fallback,
  )) as never;
const detailWith = (group: object | undefined) =>
  ({
    linkGroup: group ? { state: "present", value: group } : { state: "absent" },
  }) as unknown as SpaceDetail;
const counted = { resultVersion: "r1", itemNo: 4 };
const folder = { resultVersion: "r1", itemNo: 2 };
const client = {
  call: vi.fn().mockResolvedValue({
    displayPath: { state: "present", value: "/Users/a/clip.mp4" },
  }),
} as unknown as AnalysisClient;

function countedValue(
  countedItem: { resultVersion: string; itemNo: number },
  client: AnalysisClient,
) {
  const [row] = sharedSpaceRows(
    detailWith({ sharesSpace: true, countedItem }),
    client,
    undefined,
    fallbackText,
  );
  return row!.value;
}

function displayPath(value: string) {
  return { displayPath: { state: "present", value } };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

test("F03 #359: a path that is not counted says where it is counted and opens that folder", async () => {
  const navigate = vi.fn();
  const rows = sharedSpaceRows(
    detailWith({
      sharesSpace: true,
      countedItem: counted,
      countedFolder: folder,
    }),
    client,
    navigate,
    fallbackText,
  );
  expect(rows.map((row) => row.label)).toEqual(["Counted at"]);
  expect(rows[0]!.notes).toEqual([
    "The same file is at more than one path, so it is counted at one of them.",
  ]);
  render(<div>{rows[0]!.value}</div>);
  await waitFor(() =>
    // The path is split for the middle ellipsis, so read the whole text.
    expect(document.body.textContent).toContain("Counted at /Users/a/clip.mp4"),
  );
  fireEvent.click(screen.getByRole("button", { name: "Open folder" }));
  expect(navigate).toHaveBeenCalledWith(folder);
});

test.each([
  { resultVersion: "r2", itemNo: 4 },
  { resultVersion: "r1", itemNo: 5 },
])(
  "F03 #359: the old counted path disappears immediately when the item ref changes",
  async (next) => {
    const pending = deferred<ReturnType<typeof displayPath>>();
    const browse = {
      call: vi
        .fn()
        .mockResolvedValueOnce(displayPath("/Users/a/old.mp4"))
        .mockReturnValueOnce(pending.promise),
    } as unknown as AnalysisClient;
    const view = render(<div>{countedValue(counted, browse)}</div>);
    await waitFor(() =>
      expect(document.body.textContent).toContain("/Users/a/old.mp4"),
    );

    view.rerender(<div>{countedValue(next, browse)}</div>);
    expect(document.body.textContent).not.toContain("/Users/a/old.mp4");
    expect(document.body.textContent).toContain("Counted at");
  },
);

test.each([
  ["a failed detail lookup", () => Promise.reject(new Error("unavailable"))],
  [
    "a detail without a path",
    () => Promise.resolve({ displayPath: { state: "absent" } }),
  ],
])("F03 #359: %s keeps the default counted-path text", async (_case, reply) => {
  const browse = { call: vi.fn(reply) } as unknown as AnalysisClient;
  render(<div>{countedValue(counted, browse)}</div>);
  await waitFor(() => expect(browse.call).toHaveBeenCalledOnce());
  await waitFor(() =>
    expect(document.body.textContent).toContain("Counted at"),
  );
  expect(document.body.textContent).not.toContain("/Users/a/clip.mp4");
});

test("F03 #359: a late detail reply cannot restore the previous counted path", async () => {
  const oldReply = deferred<ReturnType<typeof displayPath>>();
  const currentReply = deferred<ReturnType<typeof displayPath>>();
  const browse = {
    call: vi
      .fn()
      .mockReturnValueOnce(oldReply.promise)
      .mockReturnValueOnce(currentReply.promise),
  } as unknown as AnalysisClient;
  const view = render(<div>{countedValue(counted, browse)}</div>);
  const next = { resultVersion: "r2", itemNo: 8 };
  view.rerender(<div>{countedValue(next, browse)}</div>);
  await waitFor(() => expect(browse.call).toHaveBeenCalledTimes(2));

  await act(async () =>
    currentReply.resolve(displayPath("/Users/a/current.mp4")),
  );
  await waitFor(() =>
    expect(document.body.textContent).toContain("/Users/a/current.mp4"),
  );
  await act(async () => oldReply.resolve(displayPath("/Users/a/old.mp4")));
  expect(document.body.textContent).toContain("/Users/a/current.mp4");
  expect(document.body.textContent).not.toContain("/Users/a/old.mp4");
});

test("F03 #359: the counted path says so without a button, and an unmarked file shows nothing", () => {
  const navigate = vi.fn();
  const [row] = sharedSpaceRows(
    detailWith({ sharesSpace: true }),
    client,
    navigate,
    fallbackText,
  );
  render(<div>{row!.value}</div>);
  expect(screen.getByText("This path")).toBeTruthy();
  expect(screen.queryByRole("button")).toBeNull();
  for (const group of [{}, undefined])
    expect(
      sharedSpaceRows(detailWith(group), client, navigate, fallbackText),
    ).toEqual([]);
});

test("F03 #359: a path that shares allocated space says so, and so does the counted one", () => {
  const rows = shown("allocated");
  expect(rows[1]).toContain(visualizationMessage("en", "sharesSpace"));
  expect(rows[0]).toContain(
    visualizationMessage("en", "sharesSpaceCountedHere"),
  );
  expect(rows[2]).not.toContain("another path");
});

test.each([["logical" as const], [undefined]])(
  "F03 #359: the shared-space fields are not read when the page basis is %s",
  (basis) => {
    const rows = shown(basis);
    expect(rows.join(" ")).not.toContain("another path");
  },
);

test("F03 #359: tooltip focus is accessible, focus and hover deduplicate, and failure can retry", async () => {
  const input = modelFixture();
  input.frame.model.representation.sizeBasis = "allocated";
  input.page.response.sizeBasis = "allocated";
  const countedItem = input.page.response.rows[0]!.itemRef!;
  input.page.response.rows[1] = {
    ...input.page.response.rows[1]!,
    sharesSpace: true,
    sharedCountedItem: countedItem,
  };
  const failed = deferred<ReturnType<typeof displayPath>>();
  const browse = {
    call: vi
      .fn()
      .mockReturnValueOnce(failed.promise)
      .mockResolvedValueOnce(displayPath("/Users/a/clip.mp4")),
  } as unknown as AnalysisClient;
  render(
    <ModelRows
      client={browse}
      page={input.page}
      model={input.frame.model}
      text={text}
      expanded
    />,
  );
  expect(browse.call).not.toHaveBeenCalled();
  const trigger = document.querySelector<HTMLElement>(
    "tbody tr:nth-child(2) td:first-child [tabindex='0']",
  )!;
  fireEvent.pointerEnter(trigger);
  trigger.focus();
  expect(document.activeElement).toBe(trigger);
  expect(browse.call).toHaveBeenCalledOnce();
  await act(async () => failed.reject(new Error("unavailable")));
  trigger.blur();
  trigger.focus();
  await waitFor(() => expect(browse.call).toHaveBeenCalledTimes(2));
  await waitFor(() =>
    expect(trigger.getAttribute("title")).toBe("Counted at /Users/a/clip.mp4"),
  );
  expect(browse.call).toHaveBeenCalledWith("browse.detail", {
    resultVersion: countedItem.resultVersion,
    item: countedItem,
  });
});

test("F03 #359: replacing the result page ignores its old tooltip reply", async () => {
  const input = modelFixture();
  input.frame.model.representation.sizeBasis = "allocated";
  input.page.response.sizeBasis = "allocated";
  input.page.response.rows[1] = {
    ...input.page.response.rows[1]!,
    sharesSpace: true,
    sharedCountedItem: { resultVersion: "r1", itemNo: 80 },
  };
  const oldReply = deferred<ReturnType<typeof displayPath>>();
  const newReply = deferred<ReturnType<typeof displayPath>>();
  const browse = {
    call: vi
      .fn()
      .mockReturnValueOnce(oldReply.promise)
      .mockReturnValueOnce(newReply.promise),
  } as unknown as AnalysisClient;
  const view = render(
    <ModelRows
      client={browse}
      page={input.page}
      model={input.frame.model}
      text={text}
      expanded
    />,
  );
  document
    .querySelector<HTMLElement>(
      "tbody tr:nth-child(2) td:first-child [tabindex='0']",
    )!
    .focus();
  expect(browse.call).toHaveBeenCalledOnce();

  const nextPage = {
    ...input.page,
    request: { ...input.page.request, resultRevision: "r2" },
    response: {
      ...input.page.response,
      resultRevision: "r2",
      rows: input.page.response.rows.map((row, index) => ({
        ...row,
        ...(row.itemRef
          ? { itemRef: { ...row.itemRef, resultVersion: "r2" } }
          : {}),
        ...(index === 1
          ? { sharedCountedItem: { resultVersion: "r2", itemNo: 81 } }
          : {}),
      })),
    },
  };
  view.rerender(
    <ModelRows
      client={browse}
      page={nextPage}
      model={input.frame.model}
      text={text}
      expanded
    />,
  );
  const currentTrigger = document.querySelector<HTMLElement>(
    "tbody tr:nth-child(2) td:first-child [tabindex='0']",
  )!;
  expect(currentTrigger.getAttribute("title")).toBeNull();
  currentTrigger.focus();
  await waitFor(() => expect(browse.call).toHaveBeenCalledTimes(2));
  await act(async () => newReply.resolve(displayPath("/Users/a/new.mp4")));
  await waitFor(() =>
    expect(currentTrigger.getAttribute("title")).toBe(
      "Counted at /Users/a/new.mp4",
    ),
  );
  await act(async () => oldReply.resolve(displayPath("/Users/a/old.mp4")));
  expect(currentTrigger.getAttribute("title")).toBe(
    "Counted at /Users/a/new.mp4",
  );
});
