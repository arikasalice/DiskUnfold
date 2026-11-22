import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { sharedSpaceRows } from "../src/features/analysis/shared-space-rows";
import type { SpaceDetail } from "../src/ipc";
import { ModelRows } from "../src/features/visualization/ModelRows";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";

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

const fallbackText = ((_key: string, fallback: string) => fallback) as never;
const detailWith = (group: object | undefined) =>
  ({
    linkGroup: group
      ? { state: "present", value: group }
      : { state: "absent" },
  }) as unknown as SpaceDetail;
const counted = { resultVersion: "r1", itemNo: 4 };

test("F03 #359: only the path that is not counted can open the counted path", () => {
  const navigate = vi.fn();
  const rows = sharedSpaceRows(
    detailWith({ sharesSpace: true, countedItem: counted }),
    navigate,
    fallbackText,
  );
  expect(rows.map((row) => row.label)).toEqual([
    "Shared allocation",
    "Counted at another path",
  ]);
  render(<div>{rows[1]!.value}</div>);
  fireEvent.click(screen.getByRole("button", { name: "Open folder" }));
  expect(navigate).toHaveBeenCalledWith(counted);
  // The counted path has nothing to follow, and an unmarked file shows nothing.
  expect(
    sharedSpaceRows(detailWith({ sharesSpace: true }), navigate, fallbackText),
  ).toHaveLength(1);
  expect(sharedSpaceRows(detailWith({}), navigate, fallbackText)).toEqual([]);
  expect(sharedSpaceRows(detailWith(undefined), navigate, fallbackText)).toEqual(
    [],
  );
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
