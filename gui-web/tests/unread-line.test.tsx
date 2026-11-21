import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";
import type { VizQuantity } from "../src/ipc";

afterEach(cleanup);

const textFor = (language: "en" | "ko"): VizText =>
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

function presenter(
  folders: VizQuantity | undefined,
  row: number | undefined,
  language: "en" | "ko" = "en",
) {
  const input = modelFixture();
  const summary = input.frame.model.summary;
  if (folders) summary.unreadFolders = folders;
  if (row !== undefined) summary.firstUnreadRow = row;
  // The third row is a folder that could not be read at all.
  input.page.response.rows[2] = {
    ...input.page.response.rows[2]!,
    size: {
      basis: "logical",
      evidence: "measured",
      state: "lower_bound",
      bytes: "0",
    },
    unread: { kind: "itself" },
  };
  input.page.response.rows[1] = {
    ...input.page.response.rows[1]!,
    unread: { kind: "inside", count: "3" },
  };
  input.page.response.rows[0] = {
    ...input.page.response.rows[0]!,
    unread: { kind: "inside" },
  };
  input.page.response.rows[3] = {
    ...input.page.response.rows[3]!,
    // A kind this screen does not know says nothing.
    unread: { kind: "later" } as never,
  };
  const onPage = vi.fn();
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
const exact = (value: string): VizQuantity => ({
  unit: "items",
  state: "exact",
  value,
});

test.each([
  ["en", "Unreadable folders: 2", "View in list"],
  ["ko", "읽지 못한 폴더 2개", "목록에서 보기"],
] as const)(
  "F06 README 48: the summary line counts the unreadable folders (%s)",
  (language, line, action) => {
    presenter(exact("2"), 2, language);
    expect(screen.getByText(line)).toBeTruthy();
    expect(
      screen.getByRole("button", { name: `${line}, ${action}` }),
    ).toBeTruthy();
  },
);

test("F06 README 48: a lower bound reads as at least, and without a row only the count stands", () => {
  presenter({ unit: "items", state: "lower_bound", value: "1" }, undefined);
  expect(screen.getByText("Unreadable folders: 1+")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /View in list$/ })).toBeNull();
  cleanup();
  presenter(undefined, undefined);
  expect(screen.queryByText(/^Unreadable folders/)).toBeNull();
});

test("F06 README 48: View in list opens the rows on the first unreadable folder, which reads unavailable", () => {
  const onPage = presenter(exact("2"), 2);
  fireEvent.click(screen.getByRole("button", { name: /View in list$/ }));
  const row = document.activeElement as HTMLElement;
  expect(row.tagName).toBe("TR");
  expect(row.getAttribute("aria-rowindex")).toBe("4");
  expect(row.textContent).toContain(
    visualizationMessage("en", "unavailableValue"),
  );
  expect(row.textContent).not.toContain("At least 0");
  expect(onPage).not.toHaveBeenCalled();
  // Each folder row says how it could not be read, as the browse screen does (README 53).
  expect(row.textContent).toContain(visualizationMessage("en", "unreadItself"));
  const rows = Array.from(document.querySelectorAll("tbody tr"));
  expect(rows[1]!.textContent).toContain("3 subfolders unreadable");
  expect(rows[0]!.textContent).toContain(
    visualizationMessage("en", "unreadInsideSome"),
  );
  expect(rows[3]!.textContent).not.toMatch(/unreadable|Couldn't read/);
});

test("F06 README 53: the reason is the tooltip and the button's description", () => {
  presenter(exact("2"), 2);
  const button = screen.getByRole("button", { name: /View in list$/ });
  const id = button.getAttribute("aria-describedby")!;
  expect(document.getElementById(id)?.textContent).toBe(
    visualizationMessage("en", "unreadTooltip"),
  );
  expect(button.closest(".du-undrawn-line")?.getAttribute("title")).toBe(
    visualizationMessage("en", "unreadTooltip"),
  );
});
