import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { ModelRows } from "../src/features/visualization/ModelRows";
import { createProductionVizText, visualizationMessage } from "../src/viz";

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
    input.page.response.rows[0]!.displayFlags = ["viz.flag.lower_bound"];
  });
  expect(headers()).toContain("Notes");
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
