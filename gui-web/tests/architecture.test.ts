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

test("rejects forbidden dependencies while accepting a local IPC client", () => {
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

    write("src/ipc/index.ts", 'export { invoke } from "@tauri-apps/api/core";');
    write("src/features/a/entry.ts", 'export { invoke } from "../../ipc";');
    write("src/features/b/entry.ts", "export const value = 1;");
    expect(check().status).toBe(0);

    write(
      "src/features/a/entry.ts",
      'export { invoke } from "@tauri-apps/api/core";\nexport { value } from "../b/entry";',
    );
    write(
      "src/features/b/entry.ts",
      'export { invoke } from "../a/entry";\nexport const value = 1;',
    );
    const failed = check();
    expect(failed.status).not.toBe(0);
    for (const rule of [
      "tauri-only-in-ipc",
      "feature-isolation",
      "no-circular",
    ]) {
      expect(failed.output).toContain(rule);
    }
  } finally {
    rmSync(sandbox, { recursive: true, force: true });
  }
});
