import {
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
