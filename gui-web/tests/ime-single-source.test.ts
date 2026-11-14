import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";

function sources(folder: string): string[] {
  return readdirSync(folder, { withFileTypes: true }).flatMap((entry) => {
    const path = join(folder, entry.name);
    if (entry.isDirectory())
      return entry.name === "generated" ? [] : sources(path);
    return /\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

// FR10.HD1: the composition decision lives in one module. Every other file asks
// it through isImeProcessingKey instead of reading the composition signals.
test("only ime-composition.ts reads composition signals", () => {
  const offenders = sources("src").filter((path) => {
    if (path.replaceAll("\\", "/").endsWith("components/ime-composition.ts"))
      return false;
    return /\.isComposing\b|keyCode\s*[!=]==?\s*229|"Process"/.test(
      readFileSync(path, "utf8"),
    );
  });
  expect(offenders).toEqual([]);
});
