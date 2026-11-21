import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import type { VizQuantity } from "../src/ipc";

afterEach(cleanup);

type Language = "en" | "ko";
const textFor = (language: Language): VizText =>
  createProductionVizText(
    (key, values) =>
      Object.entries(values ?? {}).reduce(
        (message, [name, value]) => message.replace(`{${name}}`, String(value)),
        visualizationMessage(language, key),
      ),
    { system: "iec", unit: "auto" },
  );
const view = {
  viewport: { width: 960, height: 330, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
};
const items = (value: string): VizQuantity => ({
  state: "exact",
  unit: "items",
  value,
});

function presenter(
  edit: (input: ReturnType<typeof modelFixture>) => void,
  language: Language = "en",
  onPage = vi.fn(),
) {
  const input = modelFixture();
  edit(input);
  render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      onPage={onPage}
      text={textFor(language)}
    />,
  );
  return onPage;
}

const undrawn =
  (row: number, size?: VizQuantity) =>
  (input: ReturnType<typeof modelFixture>) => {
    input.frame.model.summary.unknownItems = items("3");
    input.frame.model.summary.firstUnknownRow = row;
    if (size) input.frame.model.summary.unknownSize = size;
  };

test.each([
  ["en", /^Items not drawn: 3 · at least \d/, "View in list"],
  ["ko", /^그리지 않은 항목 3개 · 최소 \d/, "목록에서 보기"],
] as const)(
  "the summary line counts the items not drawn with their size, and says why (%s)",
  (language, line, action) => {
    presenter(
      undrawn(2, { state: "lower_bound", unit: "bytes", value: "1258291" }),
      language,
    );
    const words = screen.getByText(line);
    expect(words.closest(".du-undrawn-line")?.getAttribute("title")).toBe(
      visualizationMessage(language, "undrawnTooltip"),
    );
    const button = screen.getByRole("button", {
      name: `${words.textContent}, ${action}`,
    });
    expect(button.textContent).toBe(action);
  },
);

test("an unknown size leaves only the count, and no row leaves no line", () => {
  presenter(
    undrawn(2, {
      state: "unknown",
      unit: "bytes",
      reasonKey: "space.data_insufficient",
    }),
  );
  expect(screen.getByText("Items not drawn: 3")).toBeTruthy();
  cleanup();
  presenter((input) => {
    input.frame.model.summary.unknownItems = items("3");
  });
  expect(screen.queryByText(/^Items not drawn: 3/)).toBeNull();
});

test("View in list opens the folded rows and focuses the first item not drawn", () => {
  const onPage = presenter(undrawn(2));
  const rows = visualizationMessage("en", "rowsFromGraph");
  expect(
    screen.getByRole("button", { name: rows }).getAttribute("aria-expanded"),
  ).toBe("false");
  fireEvent.click(screen.getByRole("button", { name: /View in list$/ }));
  expect(
    screen.getByRole("button", { name: rows }).getAttribute("aria-expanded"),
  ).toBe("true");
  const focused = document.activeElement;
  expect(focused?.tagName).toBe("TR");
  // The header is row 1, so the third row of the page is row 4.
  expect(focused?.getAttribute("aria-rowindex")).toBe("4");
  expect(onPage).not.toHaveBeenCalled();
});

test("a row on another page asks for that page", () => {
  const onPage = presenter(undrawn(45));
  fireEvent.click(screen.getByRole("button", { name: /View in list$/ }));
  expect(onPage).toHaveBeenCalledWith(
    expect.objectContaining({ start: 40, count: 20 }),
  );
});

test("the button carries the sentence of the tooltip as its description", () => {
  presenter(undrawn(2));
  const button = screen.getByRole("button", { name: /View in list$/ });
  const id = button.getAttribute("aria-describedby");
  expect(id).toBeTruthy();
  expect(document.getElementById(id!)?.textContent).toBe(
    visualizationMessage("en", "undrawnTooltip"),
  );
});

test("a row the type does not apply to reads a dash, named No category", () => {
  presenter((input) => {
    undrawn(2)(input);
    input.frame.model.representation.colorCriterion = "type";
    input.page.response.rows[2]!.legendClassKey = "unclassified";
  });
  fireEvent.click(screen.getByRole("button", { name: /View in list$/ }));
  const row = document.activeElement as HTMLElement;
  expect(row.textContent).toContain("—");
  expect(row.textContent).toContain(visualizationMessage("en", "noCategory"));
  expect(
    screen.queryByText(visualizationMessage("en", "legend.unclassified")),
  ).toBeNull();
});
