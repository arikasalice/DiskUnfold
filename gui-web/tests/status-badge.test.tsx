import axe from "axe-core";
import { readFileSync } from "node:fs";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { StatusBadge, defineStatusRow, STATUS_TONES } from "../src/components";
import type { StatusTone } from "../src/components";
import { LocaleTextContext } from "../src/i18n/presentation";

afterEach(cleanup);

const tones = Object.keys(STATUS_TONES) as StatusTone[];

test("each tone has its own shape and a colour token defined in the theme", () => {
  const tokens = readFileSync("src/theme/tokens.css", "utf8");
  expect(new Set(tones.map((tone) => STATUS_TONES[tone].shape)).size).toBe(
    tones.length,
  );
  for (const tone of tones) {
    const { colorVar } = STATUS_TONES[tone];
    expect(tokens).toContain(`${colorVar}:`);
  }
});

test("a status row needs a message key", () => {
  expect(() =>
    defineStatusRow({ id: "x", tone: "good", messageKey: "" }),
  ).toThrow(RangeError);
  expect(
    defineStatusRow({ id: "done", tone: "good", messageKey: "ui.done" }),
  ).toEqual({
    id: "done",
    tone: "good",
    messageKey: "ui.done",
    shape: "check",
    colorToken: "--du-good-text",
  });
});

test("a badge shows shape and text for every tone, so colour is never the only cue", () => {
  render(
    <>
      {tones.map((tone) => (
        <StatusBadge key={tone} tone={tone}>
          {`State ${tone}`}
        </StatusBadge>
      ))}
    </>,
  );
  for (const tone of tones) {
    const badge = screen.getByText(`State ${tone}`);
    expect(badge.getAttribute("data-status-shape")).toBe(
      STATUS_TONES[tone].shape,
    );
    const glyph = badge.querySelector("svg");
    expect(glyph?.getAttribute("aria-hidden")).toBe("true");
    expect(badge.className).toContain(`du-badge--${tone}`);
  }
});

test("a status row resolves its name from the message key", async () => {
  const row = defineStatusRow({
    id: "failed",
    tone: "danger",
    messageKey: "ui.history.state_failed",
  });
  const { container } = render(
    <LocaleTextContext.Provider
      value={(key) => (key === "ui.history.state_failed" ? "실패" : key)}
    >
      <StatusBadge status={row} />
    </LocaleTextContext.Provider>,
  );
  expect(screen.getByText("실패").getAttribute("data-status-shape")).toBe(
    "cross",
  );
  expect((await axe.run(container)).violations).toEqual([]);
});
