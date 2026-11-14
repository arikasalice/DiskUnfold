import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/*
 * Reads the application's stylesheets as plain rules for tests that check
 * layout at the level of style rules: the DOM test environment computes no
 * layout, so the rendered result is verified in a real webview.
 */

const source = fileURLToPath(new URL("../src/", import.meta.url));

export type Rule = {
  file: string;
  selector: string;
  declarations: Map<string, string>;
};

export function stylesheets(): string[] {
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

export const allRules = (): Rule[] => stylesheets().flatMap(rules);

/** Rules with a selector in their list that matches the pattern. */
export const targets = (all: readonly Rule[], pattern: RegExp) =>
  all.filter((rule) =>
    rule.selector.split(",").some((part) => pattern.test(part.trim())),
  );
