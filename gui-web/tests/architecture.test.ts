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

/** Runs dependency-cruiser over a throwaway source tree with the real configuration. */
function inSandbox(body: (tree: ReturnType<typeof sandboxTree>) => void) {
  const sandbox = mkdtempSync(join(web, ".arch-fixture-"));
  try {
    for (const config of [".dependency-cruiser.cjs", "tsconfig.json"]) {
      copyFileSync(join(web, config), join(sandbox, config));
    }
    body(sandboxTree(sandbox));
  } finally {
    rmSync(sandbox, { recursive: true, force: true });
  }
}

function sandboxTree(sandbox: string) {
  function write(path: string, source: string) {
    const destination = join(sandbox, path);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, source);
  }

  function check() {
    const result = spawnSync(
      process.execPath,
      [
        join(web, "node_modules/dependency-cruiser/bin/dependency-cruiser.mjs"),
        "--config",
        ".dependency-cruiser.cjs",
        "src",
      ],
      { cwd: sandbox, encoding: "utf8" },
    );
    if (result.error) throw result.error;
    return { status: result.status, output: result.stdout + result.stderr };
  }

  return { write, check };
}

// Each fixture starts separate dependency-cruiser processes, which take far
// longer than 20 seconds when the whole suite shares a few cores.
test("enforces GUI dependency boundaries", { timeout: 60_000 }, () => {
  inSandbox(({ write, check }) => {
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
  });
});

test(
  "keeps the visualization model layer between components and features",
  { timeout: 60_000 },
  () => {
    inSandbox(({ write, check }) => {
      write("src/ipc/index.ts", "export const call = () => undefined;");
      write("src/components/index.ts", "export const Part = () => null;");
      write("src/i18n/presentation.ts", "export const text = () => '';");
      write("src/viz/model.ts", "export const model = 1;");
      write(
        "src/viz/index.ts",
        [
          'export { model } from "./model";',
          'export { call } from "../ipc";',
          'export { Part } from "../components";',
          'export { text } from "../i18n/presentation";',
        ].join("\n"),
      );
      write("src/features/a/entry.ts", 'export { model } from "../../viz";');
      write("src/app/entry.ts", 'export { model } from "../viz";');
      expect(check().status).toBe(0);

      write("src/features/a/screen.ts", "export const screen = 1;");
      write("src/app/shell.ts", "export const shell = 1;");
      write("src/state/store.ts", "export const store = 1;");
      write(
        "src/viz/up.ts",
        [
          'export { screen } from "../features/a/screen";',
          'export { shell } from "../app/shell";',
          'export { store } from "../state/store";',
        ].join("\n"),
      );
      write("src/components/down.ts", 'export { model } from "../viz";');
      write("src/state/down.ts", 'export { model } from "../viz";');
      write("src/ipc/down.ts", 'export { model } from "../viz";');
      write(
        "src/features/a/deep.ts",
        'export { model } from "../../viz/model";',
      );
      const failed = check();
      expect(failed.status).not.toBe(0);
      for (const rule of [
        "viz-layer-direction",
        "viz-above-lower-layers",
        "viz-via-index",
      ]) {
        expect(failed.output).toContain(rule);
      }
      // Every violating file is named, so one rule cannot hide another.
      for (const file of [
        "src/viz/up.ts",
        "src/components/down.ts",
        "src/state/down.ts",
        "src/ipc/down.ts",
        "src/features/a/deep.ts",
      ]) {
        expect(failed.output).toContain(file);
      }
    });
  },
);
