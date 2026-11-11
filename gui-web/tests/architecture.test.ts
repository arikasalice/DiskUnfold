// @vitest-environment node
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

const web = fileURLToPath(new URL("../", import.meta.url));

// Both fixtures start a separate dependency-cruiser process.
test("enforces GUI dependency boundaries", { timeout: 20_000 }, () => {
  const sandbox = mkdtempSync(join(web, ".arch-fixture-"));
  try {
    for (const config of [".dependency-cruiser.cjs", "tsconfig.json"]) {
      copyFileSync(join(web, config), join(sandbox, config));
    }

    function write(path: string, source: string) {
      const destination = join(sandbox, path);
      mkdirSync(dirname(destination), { recursive: true });
      writeFileSync(destination, source);
    }

    function check() {
      const result = spawnSync(
        process.execPath,
        [
          join(
            web,
            "node_modules/dependency-cruiser/bin/dependency-cruiser.mjs",
          ),
          "--config",
          ".dependency-cruiser.cjs",
          "src",
        ],
        { cwd: sandbox, encoding: "utf8" },
      );
      if (result.error) throw result.error;
      return { status: result.status, output: result.stdout + result.stderr };
    }

    write("src/ipc/bridge.ts", "export const invoke = () => undefined;");
    write("src/ipc/index.ts", 'export { invoke } from "./bridge";');
    write("src/features/a/entry.ts", 'export { invoke } from "../../ipc";');
    write("src/features/b/entry.ts", "export const value = 1;");
    expect(check().status).toBe(0);

    write(
      "src/features/a/entry.ts",
      'export { invoke } from "../../ipc/bridge";\nexport { value } from "../b/entry";',
    );
    write("src/features/a/cycle-a.ts", 'import "./cycle-b";');
    write("src/features/a/cycle-b.ts", 'import "./cycle-a";');
    write("preview/fixture.ts", "export const synthetic = true;");
    write(
      "src/preview-leak.ts",
      'export { synthetic } from "../preview/fixture";',
    );
    const failed = check();
    expect(failed.status).not.toBe(0);
    for (const rule of [
      "native-bridge-only-in-ipc",
      "feature-isolation",
      "no-circular",
      "no-preview-in-product",
    ]) {
      expect(failed.output).toContain(rule);
    }
  } finally {
    rmSync(sandbox, { recursive: true, force: true });
  }
});
