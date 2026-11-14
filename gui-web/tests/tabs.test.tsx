import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { Tabs } from "../src/components";

afterEach(cleanup);

type Key = "a" | "b" | "c";
const ITEMS = [
  { key: "a", label: "Alpha", count: "3", description: "The first tab" },
  { key: "b", label: "Beta", count: "0" },
  { key: "c", label: "Gamma" },
] as const;

function Harness({ onChange }: { onChange?: (key: Key) => void }) {
  const [value, setValue] = useState<Key>("a");
  return (
    <Tabs
      label="Letters"
      items={ITEMS}
      value={value}
      onChange={(key) => {
        onChange?.(key);
        setValue(key);
      }}
    >
      <p>Panel of {value}</p>
    </Tabs>
  );
}

test("the tabs part exposes a tab list, tabs and one labelled panel", () => {
  render(<Harness />);
  const list = screen.getByRole("tablist", { name: "Letters" });
  const tabs = screen.getAllByRole("tab");
  expect(tabs.map((tab) => tab.getAttribute("aria-label"))).toEqual([
    "Alpha",
    "Beta",
    "Gamma",
  ]);
  expect(list.contains(tabs[0]!)).toBe(true);
  expect(screen.queryByRole("radio")).toBeNull();
  expect(tabs.map((tab) => tab.getAttribute("aria-selected"))).toEqual([
    "true",
    "false",
    "false",
  ]);
  const panel = screen.getByRole("tabpanel");
  expect(panel.getAttribute("aria-labelledby")).toBe(tabs[0]!.id);
  expect(panel.textContent).toBe("Panel of a");
  // The count is drawn in the tab and read as its description with the extra text.
  expect(tabs[0]!.textContent).toContain("3");
  const described = (tabs[0]!.getAttribute("aria-describedby") ?? "").split(
    " ",
  );
  expect(
    described.map((id) => document.getElementById(id)?.textContent),
  ).toEqual(["3", "The first tab"]);
  expect(tabs[2]!.getAttribute("aria-describedby")).toBeNull();
});

test("Left, Right, Home and End move between tabs and select the focused one", () => {
  const onChange = vi.fn();
  render(<Harness onChange={onChange} />);
  const [alpha, beta, gamma] = screen.getAllByRole("tab") as [
    HTMLElement,
    HTMLElement,
    HTMLElement,
  ];
  alpha.focus();
  fireEvent.keyDown(alpha, { key: "ArrowRight" });
  expect(document.activeElement).toBe(beta);
  expect(beta.getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("tabpanel").textContent).toBe("Panel of b");
  fireEvent.keyDown(beta, { key: "End" });
  expect(document.activeElement).toBe(gamma);
  expect(screen.getByRole("tabpanel").getAttribute("aria-labelledby")).toBe(
    gamma.id,
  );
  fireEvent.keyDown(gamma, { key: "ArrowLeft" });
  expect(document.activeElement).toBe(beta);
  fireEvent.keyDown(beta, { key: "Home" });
  expect(document.activeElement).toBe(alpha);
  expect(onChange.mock.calls.map(([key]) => key)).toEqual(["b", "c", "b", "a"]);
});

test("pressing a tab selects it and only the selected tab is in the tab order", () => {
  const onChange = vi.fn();
  render(<Harness onChange={onChange} />);
  const [alpha, , gamma] = screen.getAllByRole("tab") as [
    HTMLElement,
    HTMLElement,
    HTMLElement,
  ];
  expect(alpha.tabIndex).toBe(0);
  expect(gamma.tabIndex).toBe(-1);
  fireEvent.mouseDown(gamma);
  fireEvent.mouseUp(gamma);
  fireEvent.click(gamma);
  expect(onChange).toHaveBeenCalledWith("c");
  expect(gamma.getAttribute("aria-selected")).toBe("true");
});
