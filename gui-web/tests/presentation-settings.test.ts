import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import {
  APP_SCALES,
  REDUCED_MOTION_QUERY,
  applyPresentation,
  resolveMotionLevel,
  scaleFactor,
} from "../src/theme/presentation-settings";

/** A stand-in for the WebView's media query that tests can flip. */
function fakeWindow(initial: boolean) {
  const listeners = new Set<() => void>();
  const media = {
    matches: initial,
    addEventListener: (_type: string, listener: () => void) =>
      listeners.add(listener),
    removeEventListener: (_type: string, listener: () => void) =>
      listeners.delete(listener),
  };
  const queries: string[] = [];
  return {
    view: {
      matchMedia: (query: string) => {
        queries.push(query);
        return media as unknown as MediaQueryList;
      },
    },
    queries,
    set(matches: boolean) {
      media.matches = matches;
      for (const listener of listeners) listener();
    },
    listeners,
  };
}

function root() {
  return document.createElement("div");
}

test.each([
  [false, "follow_os", "standard"],
  [true, "follow_os", "reduce"],
  [false, "reduce", "reduce"],
  [true, "reduce", "reduce"],
] as const)(
  "OS reduce=%s with app setting %s gives %s",
  (os, setting, expected) => {
    expect(resolveMotionLevel(setting, os)).toBe(expected);
    const element = root();
    const win = fakeWindow(os);
    applyPresentation(element, { motion: setting, appScale: 100 }, win.view);
    expect(element.dataset.duMotion).toBe(expected);
    expect(win.queries).toEqual([REDUCED_MOTION_QUERY]);
  },
);

test("an OS state that cannot be read counts as off", () => {
  const element = root();
  applyPresentation(element, { motion: "follow_os", appScale: 100 }, undefined);
  expect(element.dataset.duMotion).toBe("standard");
  applyPresentation(
    element,
    { motion: "follow_os", appScale: 100 },
    {
      matchMedia: () => {
        throw new Error("unsupported");
      },
    },
  );
  expect(element.dataset.duMotion).toBe("standard");
  applyPresentation(element, { motion: "reduce", appScale: 100 }, undefined);
  expect(element.dataset.duMotion).toBe("reduce");
});

test("an OS change is applied without restart and cleanup removes it", () => {
  const element = root();
  const win = fakeWindow(false);
  const stop = applyPresentation(
    element,
    { motion: "follow_os", appScale: 150 },
    win.view,
  );
  expect(element.dataset.duMotion).toBe("standard");
  win.set(true);
  expect(element.dataset.duMotion).toBe("reduce");
  win.set(false);
  expect(element.dataset.duMotion).toBe("standard");
  stop();
  expect(win.listeners.size).toBe(0);
  expect(element.dataset.duMotion).toBeUndefined();
  expect(element.style.getPropertyValue("--du-app-scale")).toBe("");
});

test("each selectable scale sets the one size token; other values mean 100%", () => {
  expect(APP_SCALES).toEqual([100, 110, 125, 150, 175, 200]);
  for (const percent of APP_SCALES) {
    const element = root();
    applyPresentation(
      element,
      { motion: "follow_os", appScale: percent },
      fakeWindow(false).view,
    );
    expect(element.style.getPropertyValue("--du-app-scale")).toBe(
      String(percent / 100),
    );
  }
  expect(scaleFactor(130)).toBe(1);
  expect(scaleFactor(0)).toBe(1);
});

test("the theme reads the motion level and scale from one place each", () => {
  const locale = readFileSync("src/theme/locale.css", "utf8");
  expect(locale).toMatch(/zoom: var\(--du-app-scale, 1\)/);
  expect(locale).toMatch(
    /:root\[data-du-motion="reduce"\] \{\s*--du-state-duration: 0ms;/,
  );
  expect(locale).toMatch(/animation-duration: 0ms !important/);
  expect(locale).toMatch(/transition-duration: 0ms !important/);
});
