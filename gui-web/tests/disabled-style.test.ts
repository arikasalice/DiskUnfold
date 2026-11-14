import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const tokens = readFileSync("src/theme/tokens.css", "utf8");
const workspace = readFileSync("src/theme/workspace.css", "utf8");

function block(css: string, pattern: RegExp): string {
  const found = css.match(pattern)?.[0];
  expect(found, String(pattern)).toBeDefined();
  return found ?? "";
}

function token(css: string, name: string): string {
  const value = css.match(new RegExp(`--du-${name}: (#[0-9a-f]{6});`))?.[1];
  expect(value, name).toBeDefined();
  return value ?? "";
}

function luminance(hex: string): number {
  const [r = 0, g = 0, b = 0] = [1, 3, 5].map((at) => {
    const c = parseInt(hex.slice(at, at + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((hi ?? 0) + 0.05) / ((lo ?? 0) + 0.05);
}

const light = block(tokens, /^:root \{[^}]*\}/m);
const themes = {
  light: (name: string) => token(light, name),
  "dark (system)": (name: string) =>
    token(
      block(tokens, /:root:where\(:not\(\[data-theme="light"\]\)\) \{[^}]*\}/),
      name,
    ),
  "dark (chosen)": (name: string) =>
    token(
      block(tokens, /^:root:where\(\[data-theme="dark"\]\) \{[^}]*\}/m),
      name,
    ),
};

test.each(Object.entries(themes))(
  "disabled text reaches 3:1 on every surface a control sits on (%s)",
  (_theme, read) => {
    const text = read("disabled-text");
    // The pressed surface is left out: no rule draws disabled text on it.
    // Every pressed-surface rule excludes [data-disabled] or uses :enabled,
    // and the focusable aria-disabled actions that can still be pressed (the
    // selection action bar and the groups entry) override their background
    // with --du-disabled-surface at a higher specificity.
    for (const surface of [
      "surface",
      "page",
      "subtle",
      "hover-surface",
      "selected-surface",
    ]) {
      expect(contrast(text, read(surface)), surface).toBeGreaterThanOrEqual(3);
    }
  },
);

test("a quiet action draws no border when disabled, as when enabled", () => {
  expect(block(workspace, /^\.du-action--quiet \{[^}]*\}/m)).toMatch(
    /border-color: transparent/,
  );
  // Same specificity as the shared disabled rule, so it must come after it.
  const shared = workspace.indexOf(".du-action[data-disabled],\n");
  const quiet = workspace.search(
    /^\.du-action--quiet\[data-disabled\] \{[^}]*border-color: transparent;/m,
  );
  expect(shared).toBeGreaterThan(-1);
  expect(quiet).toBeGreaterThan(shared);
});
