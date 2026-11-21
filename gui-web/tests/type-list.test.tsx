import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { locationOf, TypeList } from "../src/features/visualization/TypeList";
import type {
  TypeListRequest,
  TypeListSource,
} from "../src/features/visualization/TypeList";
import type {
  QueryCondition,
  SearchQueryPage,
  SearchQueryRow,
} from "../src/ipc";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const text: VizText = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) => message.replace(`{${name}}`, String(value)),
      visualizationMessage("ko", key),
    ),
  { system: "iec", unit: "auto" },
);
const say = (key: Parameters<typeof visualizationMessage>[1]) =>
  visualizationMessage("ko", key);

const version = "01890f7e-8b5a-7cc0-8000-000000000001";
const scope = { resultVersion: version, itemNo: 0 };
const video: QueryCondition = {
  kind: "predicate",
  predicate: {
    kind: "registered",
    id: "pred.file_type",
    revision: 1,
    operand: "dHlwZS52aWRlbw==",
  },
};
const summary = {
  count: { unit: "items", state: "exact", value: "2" },
  size: { unit: "bytes", state: "exact", value: "10485760" },
  sizeBasis: "allocated",
} as const;

function file(itemNo: number, name: string, path: string, bytes: string) {
  const size = { evidence: "measured", state: "exact", bytes } as const;
  return {
    item: { resultVersion: version, itemNo },
    displayName: { state: "present", value: name },
    displayPath: { state: "present", value: path },
    kind: "file",
    logicalSize: { state: "present", value: { ...size, basis: "logical" } },
    allocatedSize: { state: "present", value: { ...size, basis: "allocated" } },
    modifiedAt: { state: "absent" },
    createdAt: { state: "absent" },
    missingFields: [],
    reasonKeys: [],
  } satisfies SearchQueryRow;
}

function page(rows: SearchQueryRow[], nextCursor: string | null = null) {
  return {
    rowKind: "search",
    queryRef: version,
    resultVersion: version,
    rows,
    total: { state: "exact", count: rows.length },
    unknownCount: 0,
    observedAt: null,
    completeness: "complete",
    nextCursor,
  } satisfies SearchQueryPage;
}

const ROOT = "D:\\data\\type-narrowing-scope";
const rows = [
  file(3, "talk.mp4", `${ROOT}\\docs\\talk.mp4`, "4194304"),
  file(1, "clip.mp4", `${ROOT}\\clip.mp4`, "6291456"),
];

function source(open: TypeListSource["open"]): TypeListSource & {
  open: ReturnType<typeof vi.fn>;
} {
  return { open: vi.fn(open), next: vi.fn() };
}

function list(
  from: TypeListSource,
  extra: Partial<Parameters<typeof TypeList>[0]> = {},
) {
  return render(
    <TypeList
      source={from}
      resultVersion={version}
      scope={scope}
      condition={video}
      type={say("legend.type.video")}
      summary={summary}
      text={text}
      onBack={vi.fn()}
      onClear={vi.fn()}
      {...extra}
    />,
  );
}

test("F06 README 46.13: the list opens below the chart's folder, largest first, and says what it shows", async () => {
  const from = source(async () => ({ page: page(rows), scopePath: ROOT }));
  list(from);
  expect(document.activeElement?.textContent).toBe(say("listTitle"));
  const request = from.open.mock.calls[0]![0] as TypeListRequest;
  expect(request).toMatchObject({
    resultVersion: version,
    scope,
    condition: video,
  });
  expect(request.sortKeys).toEqual([
    { id: "sort.size", revision: 1, descending: true, sizeBasis: "allocated" },
  ]);
  // Locations start at the scope folder's name; a file directly in it shows just that name.
  expect(await screen.findByText("type-narrowing-scope\\docs")).toBeTruthy();
  expect(screen.getByText("type-narrowing-scope")).toBeTruthy();
  expect(screen.getByRole("status").textContent).toBe(
    `${say("legend.type.video")} 2개를 표시합니다`,
  );
  // The total line ends with the size basis, as the legend row it came from.
  expect(
    screen.getByText(`${say("legend.type.video")} 2개 · 10.0 MiB · 할당 크기`),
  ).toBeTruthy();
  // The date column does not sort; the others do.
  expect(
    screen.queryByRole("button", { name: say("listColumnModified") }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /^이름/ }));
  await act(async () => {});
  expect((from.open.mock.calls[1]![0] as TypeListRequest).sortKeys).toEqual([
    { id: "sort.name", revision: 1, descending: false },
  ]);
});

test("F06 README 46.13: a slow list says it is building and its cancel goes back to the chart", async () => {
  vi.useFakeTimers();
  const onBack = vi.fn();
  const from = source(
    (_, signal) =>
      new Promise((_resolve, reject) =>
        signal.addEventListener("abort", () => reject(new Error("aborted"))),
      ),
  );
  list(from, { onBack });
  expect(screen.queryByText(say("listBuilding"))).toBeNull();
  await act(async () => {
    vi.advanceTimersByTime(300);
  });
  expect(screen.getByText(say("listBuilding"))).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: say("listCancel") }));
  expect(onBack).toHaveBeenCalledTimes(1);
});

test("F06 README 46.13: an empty list and the chip's × are worded as designed", async () => {
  const onClear = vi.fn();
  const from = source(async () => ({ page: page([]), scopePath: ROOT }));
  list(from, { onClear });
  expect(
    await screen.findByText(
      `이 범위에는 「${say("legend.type.video")}」에 해당하는 파일이 없습니다`,
    ),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: say("filterClearType") }));
  expect(onClear).toHaveBeenCalledTimes(1);
});

test("locations are display text from the scope folder's name on", () => {
  expect(locationOf("C:\\a\\scope\\x\\f.mp4", "C:\\a\\scope")).toBe("scope\\x");
  expect(locationOf("/home/u/scope/f.mp4", "/home/u/scope")).toBe("scope");
  // A volume root keeps its whole path; an unknown scope path shows the whole parent.
  expect(locationOf("C:\\x\\f.mp4", "C:\\")).toBe("C:\\x");
  expect(locationOf("C:\\a\\f.mp4", null)).toBe("C:\\a");
});
