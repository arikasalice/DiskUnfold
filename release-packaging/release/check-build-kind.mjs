// Confirms the build kind and commit embedded in the newest launcher build
// (F15 FR5.HD1.app-host.DD1). An official build must also carry both data format versions and a
// support-matrix revision. With --binary it also runs the built executable's `version` command and
// compares its output with the build (release stage 3, version output comparison). Usage:
//   node check-build-kind.mjs --expected official|developer --commit <40 hex> [--target-dir DIR]
//     [--binary PATH]
import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import os from "node:os";
import path from "node:path";

function findProductInfos(directory, found = []) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      findProductInfos(full, found);
    } else if (
      entry.name === "product-info.json" &&
      /du-launcher-[0-9a-f]+[\\/]out$/.test(directory)
    ) {
      found.push(full);
    }
  }
  return found;
}

/** Returns a list of problems; an empty list means the embedded info matches. */
export function checkProductInfo(info, expectedKind, commit) {
  const problems = [];
  const build = info?.build ?? {};
  if (build.buildKind !== expectedKind) {
    problems.push(`buildKind is ${build.buildKind}, expected ${expectedKind}`);
  }
  if (build.commit !== commit) {
    problems.push(`commit is ${build.commit}, expected ${commit}`);
  }
  if (expectedKind === "official") {
    for (const field of ["appDbFormat", "snapshotFormat", "supportMatrixRevision", "builtAtUtc"]) {
      if (build[field] == null) problems.push(`${field} is missing in an official build`);
    }
    if (info?.notices?.thirdParty == null) problems.push("third-party notices are missing");
  }
  return problems;
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function targetDirectory() {
  const given = argument("--target-dir");
  if (given) return given;
  const metadata = execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1", "--locked"], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  return JSON.parse(metadata).target_directory;
}

/** Returns a list of problems in the English output of `diskunfold version`. */
export function checkVersionOutput(output, expectedKind, commit) {
  const problems = [];
  const label = expectedKind === "official" ? "Official release build" : "Developer build";
  if (!output.includes(label)) problems.push(`version output does not show "${label}"`);
  if (!output.includes(commit)) problems.push(`version output does not show commit ${commit}`);
  return problems;
}

function versionOutput(binary) {
  const data = mkdtempSync(path.join(os.tmpdir(), "du-version-"));
  try {
    return execFileSync(binary, ["--lang", "en", "version"], {
      encoding: "utf8",
      env: { ...process.env, DISKUNFOLD_DATA_DIR: data },
    });
  } finally {
    rmSync(data, { recursive: true, force: true });
  }
}

if (process.argv[1]?.endsWith("check-build-kind.mjs")) {
  const expected = argument("--expected");
  const commit = argument("--commit");
  if (!["official", "developer"].includes(expected) || !commit) {
    console.error("usage: check-build-kind.mjs --expected official|developer --commit <sha> [--target-dir DIR] [--binary PATH]");
    process.exit(2);
  }
  const infos = findProductInfos(targetDirectory());
  if (infos.length === 0) {
    console.error("no launcher product-info.json found; build du-launcher first");
    process.exit(1);
  }
  infos.sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  const problems = checkProductInfo(JSON.parse(readFileSync(infos[0], "utf8")), expected, commit);
  if (problems.length > 0) {
    console.error(`release.build_kind_mismatch in ${infos[0]}:\n  ${problems.join("\n  ")}`);
    process.exit(1);
  }
  console.log(`embedded build info matches: kind=${expected} commit=${commit}`);
  const binary = argument("--binary");
  if (binary) {
    const versionProblems = checkVersionOutput(versionOutput(binary), expected, commit);
    if (versionProblems.length > 0) {
      console.error(`release.build_kind_mismatch in the output of ${binary} version:\n  ${versionProblems.join("\n  ")}`);
      process.exit(1);
    }
    console.log("version output matches the build");
  }
}
