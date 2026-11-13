// Confirms the build kind and commit embedded in the newest launcher build
// (F15 FR5.HD1.app-host.DD1). An official build must also carry both data format versions and a
// support-matrix revision. Usage:
//   node check-build-kind.mjs --expected official|developer --commit <40 hex> [--target-dir DIR]
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
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

if (process.argv[1]?.endsWith("check-build-kind.mjs")) {
  const expected = argument("--expected");
  const commit = argument("--commit");
  if (!["official", "developer"].includes(expected) || !commit) {
    console.error("usage: check-build-kind.mjs --expected official|developer --commit <sha> [--target-dir DIR]");
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
}
