import axe from "axe-core";
import { readFileSync } from "node:fs";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import {
  Action,
  Breadcrumbs,
  Choice,
  ChoiceGroup,
  ClampedText,
  DataTable,
  MiddleEllipsisText,
  Notice,
  PathField,
  ReviewCheck,
  ReviewField,
  StatusBadge,
} from "../src/components";
import {
  APP_SCALES,
  applyPresentation,
} from "../src/theme/presentation-settings";

afterEach(() => {
  cleanup();
  document.documentElement.removeAttribute("style");
});

const path =
  "/Users/someone/사진/가족 여행/2026년 여름 휴가/제주도 해변 일출 사진.jpg";

function Sample() {
  return (
    <main>
      <Breadcrumbs
        label="Location"
        entries={[
          { id: "a", label: "Documents", onPress: () => {} },
          { id: "b", label: "Reports", current: true },
        ]}
      />
      <PathField
        label="Folder to analyze"
        description="Type a folder path."
        value=""
        onChange={() => {}}
        isDisabled={false}
        error="This folder cannot be read."
      />
      <ReviewField
        label="Reason"
        description="Why this change."
        value=""
        onChange={() => {}}
      />
      <ReviewCheck
        label="I reviewed the list"
        checked={false}
        onChange={() => {}}
      />
      <ChoiceGroup
        id="drives"
        label="Drives"
        value="c"
        onChange={() => {}}
        isDisabled={false}
      >
        <Choice
          value="c"
          label="C:"
          description="Local disk"
          status={{ text: "Ready", tone: "good" }}
          icon="drive"
        />
      </ChoiceGroup>
      <StatusBadge tone="warning">Partial</StatusBadge>
      <Notice tone="error">Something failed.</Notice>
      <ClampedText text="A long explanation that wraps across several lines before it is shortened for the reader." />
      <MiddleEllipsisText text={path} />
      <DataTable
        label="Items"
        scrollLabel="Items, scrollable"
        columns={["Name", "Size"]}
        rows={[{ name: "report.xlsx", size: "1 MB" }]}
        renderCells={(row) => [row.name, row.size]}
      />
      <Action onPress={() => {}}>Analyze</Action>
      <Action icon="copy" aria-label="Copy path" onPress={() => {}} />
    </main>
  );
}

test("the shared parts have no accessibility violations", async () => {
  const { container } = render(<Sample />);
  expect((await axe.run(container)).violations).toEqual([]);
});

test("an input error is linked to its entry and announced once", () => {
  render(<Sample />);
  const entry = screen.getByRole("textbox", { name: "Folder to analyze" });
  expect(entry.getAttribute("aria-invalid")).toBe("true");
  const alerts = screen.getAllByRole("alert");
  const message = alerts.find(
    (node) => node.textContent === "This folder cannot be read.",
  );
  expect(message).toBeDefined();
  expect(entry.getAttribute("aria-describedby")).toContain(message!.id);
  expect(
    alerts.filter((node) => node.textContent === "This folder cannot be read."),
  ).toHaveLength(1);
});

test("the data table's scroll region name can come from a message", () => {
  render(<Sample />);
  expect(
    screen.getByRole("region", { name: "Items, scrollable" }),
  ).toBeDefined();
});

// happy-dom computes no layout, so this covers what it can: at the smallest
// supported window and the largest scale, every primary control stays in the
// tab order and the scrolling regions that hold the rest are keyboard
// reachable. Real reflow at 1024x700 and 200% needs the native GUI check.
test.each(APP_SCALES)(
  "at 1024x700 and %s%% the primary controls remain reachable by keyboard",
  (percent) => {
    window.innerWidth = 1024;
    window.innerHeight = 700;
    const stop = applyPresentation(
      document.documentElement,
      { motion: "follow_os", appScale: percent },
      undefined,
    );
    try {
      expect(
        document.documentElement.style.getPropertyValue("--du-app-scale"),
      ).toBe(String(percent / 100));
      render(<Sample />);
      for (const name of ["Analyze", "Copy path", "Documents"]) {
        const control = screen.getByRole("button", { name });
        expect((control as HTMLButtonElement).disabled).toBe(false);
        expect(control.tabIndex).toBeGreaterThanOrEqual(0);
      }
      expect(
        screen.getByRole("textbox", { name: "Folder to analyze" }).tabIndex,
      ).toBe(0);
      expect(
        screen.getByRole("region", { name: "Items, scrollable" }).tabIndex,
      ).toBe(0);
    } finally {
      stop();
    }
  },
);

test("the scrolling regions scroll instead of clipping", () => {
  const work = readFileSync("src/theme/workspace.css", "utf8");
  const table = work.match(/\.du-table-viewport \{[^}]*\}/)?.[0] ?? "";
  expect(table).toMatch(/overflow: auto/);
  const browse = readFileSync("src/theme/browse-grid.css", "utf8");
  const frames = browse.match(/\.du-browse-grid-frame \{[^}]*\}/g) ?? [];
  expect(frames.some((rule) => /overflow: auto/.test(rule))).toBe(true);
});
