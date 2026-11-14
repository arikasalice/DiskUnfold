// @vitest-environment node
import { expect, test } from "vitest";
import { allRules } from "./style-rules";

/*
 * Reasons and descriptions are prose. Korean breaks between words there, as
 * the design review of PR #60 asked: the browser would otherwise break Korean
 * between any two syllables ("...있습니 / 다"). A word longer than the line
 * still wraps. Where the text then breaks is verified in a real webview.
 */

const all = allRules();

/** The common parts that draw reasons and descriptions. */
const prose = [
  // A row menu's labels and unavailable reasons, rendered outside the screen.
  '.du-item-menu [role="menuitem"]',
  // The selection action bar's summary, which carries its reasons, and outcome.
  ".du-selection-actions__summary",
  ".du-selection-actions__notice",
  // Text kind="muted", the description variant.
  ".du-text--muted",
  ".du-notice",
  // ReviewField and choice descriptions and field errors.
  '.du-field [slot="description"]',
  '.du-field [slot="errorMessage"]',
  ".du-choice__description",
  ".du-readout__note",
];

function wrapping(selector: string) {
  const found = all.filter((rule) =>
    rule.selector.split(",").some((part) => part.trim() === selector),
  );
  return {
    wordBreak: found.map((rule) => rule.declarations.get("word-break")),
    overflowWrap: found.map((rule) => rule.declarations.get("overflow-wrap")),
  };
}

test.each(prose)("%s keeps Korean words whole", (selector) => {
  const { wordBreak, overflowWrap } = wrapping(selector);
  expect(wordBreak, selector).toContain("keep-all");
  expect(wordBreak, selector).not.toContain("break-all");
  // Only a word longer than the line breaks inside it.
  expect(overflowWrap, selector).toContain("anywhere");
});

test("no stylesheet breaks words between any two characters", () => {
  for (const rule of all)
    expect(
      rule.declarations.get("word-break"),
      `${rule.file}: ${rule.selector}`,
    ).not.toBe("break-all");
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
