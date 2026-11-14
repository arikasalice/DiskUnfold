// @vitest-environment node
import { expect, test } from "vitest";
import { allRules } from "./style-rules";

/*
 * Korean breaks between words everywhere, as the design review of PR #60
 * asked (follow-up 2): the browser would otherwise break Korean between any
 * two syllables ("...있습니 / 다"). One default on body covers the screen and
 * everything drawn under body outside it (menus, popovers, dialogs), and
 * paths and identifiers keep their own rules. Where the text then breaks is
 * verified in a real webview.
 */

const all = allRules();

const declared = (selector: string, property: string) =>
  all
    .filter((rule) =>
      rule.selector.split(",").some((part) => part.trim() === selector),
    )
    .map((rule) => rule.declarations.get(property))
    .filter((value) => value !== undefined);

test("body keeps Korean words whole and breaks only a word longer than its line", () => {
  expect(declared("body", "word-break")).toEqual(["keep-all"]);
  // break-word, unlike anywhere, leaves the min-content width of tables and
  // flex items as it was.
  expect(declared("body", "overflow-wrap")).toEqual(["break-word"]);
});

/** The parts that repeated the rule before it became the default. */
const inheriting = [
  '.du-item-menu [role="menuitem"]',
  ".du-selection-actions__summary",
  ".du-selection-actions__notice",
  ".du-text--muted",
  '.du-field [slot="description"]',
  '.du-field [slot="errorMessage"]',
  ".du-choice__description",
  ".du-readout__note",
  ".du-clamp",
  ".du-fit__full",
  ".du-name-hint",
  ".du-info-screen",
];

test.each(inheriting)(
  "%s takes the default instead of repeating it",
  (selector) => {
    expect(declared(selector, "word-break"), selector).toEqual([]);
  },
);

test("only paths set word-break apart from body, and never to break-all", () => {
  for (const rule of all) {
    const value = rule.declarations.get("word-break");
    if (value === undefined) continue;
    expect(value, `${rule.file}: ${rule.selector}`).not.toBe("break-all");
    expect(value, `${rule.file}: ${rule.selector}`).not.toBe("normal");
  }
});

test("no menu, popover or dialog root turns the default off", () => {
  for (const rule of all.filter((candidate) =>
    /menu|popover|dialog|modal/.test(candidate.selector),
  )) {
    expect(rule.declarations.get("word-break")).toBeUndefined();
    expect(rule.declarations.get("white-space") ?? "normal").not.toBe("nowrap");
  }
});

test("paths and identifiers still break anywhere", () => {
  const rule = (selector: string) =>
    all.find((candidate) => candidate.selector === selector)?.declarations;
  expect(rule(".du-path")?.get("overflow-wrap")).toBe("anywhere");
  expect(rule(".du-readout__identifier dd")?.get("overflow-wrap")).toBe(
    "anywhere",
  );
  expect(rule(".du-text--path")?.get("overflow-wrap")).toBe("anywhere");
});

test("a size with its unit or a date with its time never wraps inside", () => {
  expect(declared(".du-unbroken", "white-space")).toEqual(["nowrap"]);
});

test("the history target, a fixed summary from the engine, is prose", () => {
  // engine-api/src/scan.rs writes "metadata: 1 root" or "metadata: N roots",
  // and app-db's valid_scope passes only those, so no user path can appear in
  // HistoryPanel's target. Muted text therefore takes the prose default.
  expect(declared(".du-text--muted", "overflow-wrap")).toEqual([]);
  expect(declared(".du-text--muted", "word-break")).toEqual([]);
});
