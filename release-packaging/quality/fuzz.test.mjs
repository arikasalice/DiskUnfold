import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const runner = fileURLToPath(new URL("./fuzz.mjs", import.meta.url));
const toolchain = "nightly-2026-10-01";
const target = { owner: "scan-engine", name: "parse", seconds: 30 };

function fixture(t, targets) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "diskunfold-fuzz-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  function write(path, content, options) {
    const file = join(root, path);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, content, options);
  }
  write(
    "release-packaging/quality/fuzz-targets.json",
    JSON.stringify({ toolchain, cargoFuzzVersion: "0.13.1", targets }),
  );
  return { root, write };
}

function invoke(root, args = [], env = process.env) {
  const processResult = spawnSync(process.execPath, [runner, ...args], {
    cwd: root, env, encoding: "utf8",
  });
  const report = JSON.parse(readFileSync(join(root, "quality-output/fuzz-report.json"), "utf8"));
  return { ...processResult, report };
}

test("an empty registry is reported as unconfigured in both modes", (t) => {
  const { root } = fixture(t, []);
  for (const args of [[], ["--check"]]) {
    const result = invoke(root, args);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(result.report, { status: "not configured", results: [] });
  }
});

test("invalid durations, escaping paths, and missing manifests fail validation", (t) => {
  for (const registration of [
    { ...target, seconds: 601 },
    { ...target, owner: "../scan-engine" },
    target,
  ]) {
    const { root } = fixture(t, [registration]);
    const result = invoke(root, ["--check"]);
    assert.equal(result.status, 1);
    assert.equal(result.report.status, "failed");
    assert.equal(result.report.results.length, 0);
    assert.match(result.report.error, /1–600 seconds|Invalid fuzz owner/);
  }
});

test("owner paths, nightly isolation, and crash reporting survive a nonzero fuzz exit", {
  skip: process.platform === "win32",
}, (t) => {
  const { root, write } = fixture(t, [target, { ...target, owner: "job-runtime" }]);
  for (const owner of ["scan-engine", "job-runtime"]) {
    write(`${owner}/Cargo.toml`, "");
    write(`${owner}/fuzz/Cargo.toml`, "");
    write(`${owner}/fuzz/Cargo.lock`, "locked");
    write(`${owner}/fuzz/parse.rs`, "");
  }
  write("bin/rustup", "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  write("bin/cargo", `#!${process.execPath}
const fs = require("node:fs");
const path = require("node:path");
const args = process.argv.slice(2);
const owner = path.basename(process.cwd());
if (args[0] === "metadata") {
  const manifest = args.at(-1);
  console.log(JSON.stringify({ packages: [{
    manifest_path: manifest,
    targets: [{name:"parse", kind:["bin"], src_path:path.join(path.dirname(manifest),"parse.rs")}],
  }] }));
} else if (args[1] === "fetch") {
  if (args[0] !== "+${toolchain}" || !args.includes("--locked")) process.exit(8);
} else if (args[1] === "fuzz") {
  if (args[0] !== "+${toolchain}" || process.env.RUSTUP_TOOLCHAIN !== "${toolchain}" ||
      process.env.CARGO_NET_OFFLINE !== "true" || args[3] !== "parse" ||
      !args[4].endsWith("/fuzz-corpus/" + owner + "/parse") ||
      args[5] !== "--" || args[6] !== "-max_total_time=30") process.exit(9);
  if (owner === "scan-engine") {
    fs.mkdirSync("fuzz/artifacts/parse", { recursive: true });
    fs.writeFileSync("fuzz/artifacts/parse/crash-fixture", "failure input");
    process.exit(1);
  }
} else if (args[1] !== "install") process.exit(10);
`, { mode: 0o755 });
  const result = invoke(root, [], { ...process.env, PATH: `${join(root, "bin")}${delimiter}${process.env.PATH}` });
  assert.equal(result.status, 1);
  assert.equal(result.report.status, "failed");
  assert.deepEqual(result.report.results.map(({ owner, status }) => [owner, status]), [
    ["scan-engine", "failed"], ["job-runtime", "passed"],
  ]);
  assert.equal(
    readFileSync(join(root, "quality-output/fuzz-crashes/scan-engine/parse/crash-fixture"), "utf8"),
    "failure input",
  );
});
