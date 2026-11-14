// @vitest-environment node
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

/*
 * The layout of label and value rows is checked at the level of the style
 * rules: the DOM test environment computes no layout, so where a value moves
 * below its label is verified in a real webview.
 */

const source = fileURLToPath(new URL("../src/", import.meta.url));

type Rule = {
  file: string;
  selector: string;
  declarations: Map<string, string>;
};

function stylesheets(): string[] {
  return (readdirSync(source, { recursive: true }) as string[])
    .filter((path) => path.endsWith(".css"))
    .map((path) => join(source, path));
}

/** Innermost rules of a stylesheet; at-rule wrappers such as @media are dropped. */
function rules(file: string): Rule[] {
  const css = readFileSync(file, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  return Array.from(css.matchAll(/([^{}]+)\{([^{}]*)\}/g), (match) => ({
    file,
    selector: match[1]!.trim().replace(/\s+/g, " "),
    declarations: new Map(
      match[2]!
        .split(";")
        .map((part) => part.split(/:(.*)/s).map((value) => value.trim()))
        .filter(([name, value]) => name && value)
        .map(([name, value]) => [name!, value!.replace(/\s+/g, " ")]),
    ),
  }));
}

const all = stylesheets().flatMap(rules);
const targets = (pattern: RegExp) =>
  all.filter((rule) =>
    rule.selector.split(",").some((part) => pattern.test(part.trim())),
  );

function shared(selector: string) {
  const found = all.find(
    (rule) => rule.file.endsWith("workspace.css") && rule.selector === selector,
  );
  expect(found, selector).toBeDefined();
  return Object.fromEntries(found!.declarations);
}

test("a label keeps its width and never wraps; the value takes the rest and wraps", () => {
  expect(shared(".du-readout > div")).toMatchObject({
    display: "flex",
    "flex-wrap": "wrap",
    "justify-content": "space-between",
  });
  expect(shared(".du-readout dt")).toMatchObject({
    flex: "none",
    "white-space": "nowrap",
  });
  // The value moves below the label when less than 12ch would remain, grows no
  // wider than its content, and starts at the left when it stands alone.
  expect(shared(".du-readout dd")).toMatchObject({
    flex: "1 1 12ch",
    "min-inline-size": "0",
    "max-inline-size": "max-content",
    "text-align": "start",
  });
});

test("no screen lets a readout label shrink or wrap", () => {
  const labels = targets(/\.du-readout\b.*\bdt$/);
  expect(labels.length).toBeGreaterThan(0);
  for (const rule of labels) {
    const where = `${rule.file}: ${rule.selector}`;
    const declarations = Object.fromEntries(rule.declarations);
    if ("white-space" in declarations)
      expect(declarations["white-space"], where).toBe("nowrap");
    if ("flex" in declarations) expect(declarations.flex, where).toBe("none");
    expect(declarations["flex-shrink"] ?? "0", where).toBe("0");
    expect(declarations["word-break"] ?? "normal", where).not.toBe("break-all");
    expect(declarations["overflow-wrap"] ?? "normal", where).toBe("normal");
  }
});

test("a screen that lays rows out in columns sizes the label column to its label", () => {
  const grids = targets(/\.du-readout > div$/).filter((rule) =>
    rule.declarations.has("grid-template-columns"),
  );
  expect(grids.length).toBeGreaterThan(0);
  for (const rule of grids) {
    const columns = rule.declarations.get("grid-template-columns")!;
    expect(
      columns === "minmax(0, 1fr)" || columns.startsWith("minmax(max-content,"),
      `${rule.file}: ${rule.selector} uses ${columns}`,
    ).toBe(true);
  }
});

test("paths break after a separator first and identifiers anywhere", () => {
  const path = all.find((rule) => rule.selector === ".du-path");
  expect(Object.fromEntries(path!.declarations)).toMatchObject({
    "word-break": "keep-all",
    "overflow-wrap": "anywhere",
  });
  expect(
    targets(/\.du-readout__identifier dd$/).every(
      (rule) => rule.declarations.get("overflow-wrap") === "anywhere",
    ),
  ).toBe(true);
});

test("the retired wide value class is gone", () => {
  for (const file of stylesheets())
    expect(readFileSync(file, "utf8"), file).not.toContain("wide-value");
});
