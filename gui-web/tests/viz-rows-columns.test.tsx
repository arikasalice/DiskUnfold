import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { ModelRows } from "../src/features/visualization/ModelRows";
import {
  createProductionVizText,
  rowRatioText,
  visualizationMessage,
} from "../src/viz";

afterEach(cleanup);

const text = createProductionVizText(
  (key, values) =>
    Object.entries(values ?? {}).reduce(
      (message, [name, value]) => message.replace(`{${name}}`, String(value)),
      visualizationMessage("en", key),
    ),
  { system: "iec", unit: "auto" },
);

function show(
  edit: (input: ReturnType<typeof modelFixture>) => void = () => {},
  props: { expanded?: boolean; onPage?: () => void } = { expanded: true },
) {
  const input = modelFixture();
  for (const row of input.page.response.rows) {
    row.legendClassKey = "unclassified";
    row.displayFlags = [];
  }
  edit(input);
  input.frame.model.shapes.forEach((shape) => (shape.displayFlags = []));
  return render(
    <ModelRows
      page={input.page}
      model={input.frame.model}
      text={text}
      {...props}
    />,
  );
}
const headers = () =>
  screen.getAllByRole("columnheader").map((header) => header.textContent);

test("the table is open on first show and is as tall as its rows, with no inner scroll area", () => {
  const { container } = show();
  expect(screen.getByRole("table")).toBeTruthy();
  const viewport = container.querySelector(".du-table-viewport")!;
  expect(viewport.classList.contains("du-table-viewport--flat")).toBe(true);
  expect(viewport.getAttribute("tabindex")).toBeNull();
});

test("the classification column waits for a classification", () => {
  show();
  expect(headers()).not.toContain("Classification");
  cleanup();
  show((input) => {
    input.page.response.rows[0]!.legendClassKey = "video";
  });
  expect(headers()).toContain("Classification");
});

test("the notes column appears only when a row or a shape of the graph has a note", () => {
  show();
  expect(headers()).not.toContain("Notes");
  cleanup();
  show((input) => {
    input.page.response.rows[0]!.displayFlags = ["viz.flag.other_group"];
  });
  expect(headers()).toContain("Notes");
  cleanup();
  // Flags already shown in the value column are not repeated as notes.
  show((input) => {
    input.page.response.rows[0]!.displayFlags = [
      "viz.flag.lower_bound",
      "viz.flag.unknown",
    ];
  });
  expect(headers()).not.toContain("Notes");
});

test("a denominator every row shares is said once above the table", () => {
  show();
  expect(headers()).not.toContain("Denominator");
  expect(screen.getByText(/^Ratio denominator: /).textContent).toContain(
    text.key(modelFixture().page.response.rows[0]!.denominator.basis),
  );
  cleanup();
  show((input) => {
    const other = input.page.response.rows[1]!;
    other.denominator = {
      ...other.denominator,
      value: { unit: "bytes", state: "exact", value: "1" },
    };
  });
  expect(headers()).toContain("Denominator");
  expect(screen.queryByText(/^Ratio denominator: /)).toBeNull();
  cleanup();
  // An unknown denominator is kept apart from its basis.
  show((input) => {
    for (const row of input.page.response.rows)
      row.denominator = {
        ...row.denominator,
        value: {
          unit: "bytes",
          state: "unknown",
          reasonKey: "fixture.unknown",
        },
      };
  });
  const basis = text.key(
    modelFixture().page.response.rows[0]!.denominator.basis,
  );
  expect(screen.getByText(/^Ratio denominator: /).textContent).toBe(
    `Ratio denominator: ${basis} · Unavailable`,
  );
});

test("the page buttons show only when there is more than one page", () => {
  show(
    (input) => {
      input.page.response.totalCount = input.page.response.rows.length;
      input.page.request.start = 0;
    },
    { expanded: true, onPage: () => {} },
  );
  expect(screen.queryByRole("button", { name: "Next rows" })).toBeNull();
  cleanup();
  show(
    (input) => {
      input.page.response.totalCount = input.page.response.rows.length + 5;
    },
    { expanded: true, onPage: () => {} },
  );
  const next = screen.getByRole("button", { name: "Next rows" });
  expect(next).toBeTruthy();
  expect(
    within(next.parentElement!).getByRole("button", { name: "Previous rows" }),
  ).toBeTruthy();
});

test.each(["lower_bound", "unknown"] as const)(
  "a row whose value is %s shows the unavailable word and no percentage",
  (state) => {
    show((input) => {
      const row = input.page.response.rows[0]!;
      const value = { state, value: "100", bytes: "100" };
      row.quantity = { unit: "bytes", ...value } as never;
      row.size = { ...row.size, ...value } as never;
      row.ratio = "0.5";
    });
    expect(screen.getAllByText("Unavailable").length).toBeGreaterThan(0);
    expect(screen.getByRole("table").textContent).not.toContain("50.0%");
  },
);

test("the ratio of an exact row is a percentage and a missing ratio is unavailable", () => {
  const exact = { ...modelFixture().page.response.rows[0]!, ratio: "0.5" };
  expect(rowRatioText(exact, text)).toBe("50.0%");
  expect(rowRatioText({ ...exact, ratio: null }, text)).toBe("Unavailable");
});

test("a small negative exact ratio keeps its sign in the row", () => {
  show((input) => {
    input.page.response.rows[0]!.ratio = "-0.00004";
  });
  expect(
    within(
      screen.getByRole("table", { name: "Items in this folder" }),
    ).getByText("Greater than -0.1% and less than 0%"),
  ).toBeTruthy();
});

test("one page says how many items there are and several pages keep the range", () => {
  show(
    (input) => {
      input.page.response.totalCount = input.page.response.rows.length;
      input.page.request.start = 0;
    },
    { expanded: true, onPage: () => {} },
  );
  expect(
    screen.getByText(`${modelFixture().page.response.rows.length} items`),
  ).toBeTruthy();
  cleanup();
  show(
    (input) => {
      input.page.response.totalCount = input.page.response.rows.length + 5;
    },
    { expanded: true, onPage: () => {} },
  );
  expect(screen.getByText(/^Rows 1–\d+ of \d+$/)).toBeTruthy();
});
