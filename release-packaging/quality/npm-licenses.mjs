import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const shipped = ["MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "0BSD", "Unicode-3.0", "Unicode-DFS-2016", "BSL-1.0", "CC0-1.0"];
const build = [...shipped, "BlueOak-1.0.0", "MPL-2.0"];

function componentLicenses(sbom) {
  if (sbom.bomFormat !== "CycloneDX" || !Array.isArray(sbom.components)) {
    throw new Error("Expected a CycloneDX component inventory");
  }
  const licenses = new Map();
  for (const component of sbom.components) {
    const key = `${component.name}@${component.version}`;
    const [license] = component.licenses ?? [];
    const expression = license?.expression ?? license?.license?.id;
    if (component.licenses?.length !== 1 || typeof expression !== "string") {
      throw new Error(`Unidentified license for ${key}`);
    }
    if (licenses.has(key) && licenses.get(key) !== expression) {
      throw new Error(`Conflicting CycloneDX licenses for ${key}`);
    }
    licenses.set(key, expression);
  }
  return licenses;
}

function matchInstalled(nodes, locked, root, seen) {
  for (const [alias, pkg] of Object.entries(nodes ?? {})) {
    // npm lists unavailable optional peers without an installed path or version.
    if (pkg.path || pkg.version) {
      if (typeof pkg.path !== "string") throw new Error(`Missing installed path for ${alias}`);
      const path = relative(root, pkg.path).split(sep).join("/");
      const expected = locked.get(path);
      if (!expected || expected.alias !== alias || expected.version !== pkg.version ||
          expected.name !== (pkg.name ?? alias) || !expected.production) {
        throw new Error(`Unexpected installed production package: ${alias} at ${path}`);
      }
      seen.add(path);
    }
    matchInstalled(pkg.dependencies, locked, root, seen);
  }
}

export function inventory(lock, sbom, installed, root) {
  const components = componentLicenses(sbom);
  const matched = new Set();
  const packages = new Map();
  for (const [path, pkg] of Object.entries(lock.packages)) {
    if (!path) continue;
    const alias = path.split("node_modules/").at(-1);
    const key = `${alias}@${pkg.version}`;
    const license = components.get(key);
    if (!path.startsWith("node_modules/") || !pkg.resolved?.startsWith("https://registry.npmjs.org/") ||
        !pkg.integrity || !license || license !== pkg.license) {
      throw new Error(`Missing or inconsistent locked license for ${key} at ${path}`);
    }
    matched.add(key);
    packages.set(path, { alias, name: pkg.name ?? alias, version: pkg.version, license,
      production: pkg.dev !== true, optional: pkg.optional === true, path });
  }
  if (matched.size !== components.size) throw new Error("CycloneDX contains unlocked packages");
  const seen = new Set();
  matchInstalled(installed.dependencies, packages, root, seen);
  for (const pkg of packages.values()) {
    if (pkg.production && !pkg.optional && !seen.has(pkg.path)) {
      throw new Error(`Missing installed production package: ${pkg.path}`);
    }
  }
  return [...packages.values()];
}

function runJson(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 32 * 1024 * 1024, ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed: ${result.stderr}`);
  return JSON.parse(result.stdout);
}

function main() {
  const root = resolve("gui-web");
  mkdirSync("quality-output", { recursive: true });
  const lock = JSON.parse(readFileSync("gui-web/package-lock.json", "utf8"));
  const options = { cwd: root, shell: process.platform === "win32" };
  const sbom = runJson("npm", ["sbom", "--sbom-format=cyclonedx", "--include=dev", "--package-lock-only"], options);
  const installed = runJson("npm", ["ls", "--all", "--omit=dev", "--json", "--long"], options);
  writeFileSync("quality-output/npm-cyclonedx.json", JSON.stringify(sbom, null, 2) + "\n");
  const packages = inventory(lock, sbom, installed, root);
  const input = JSON.stringify(packages.map(pkg => ({ expression: pkg.license, allowed: pkg.production ? shipped : build })));
  const decisions = runJson("cargo", ["run", "--quiet", "--locked", "-p", "du-release-tools", "--bin", "check-npm-licenses"], { input });
  if (!Array.isArray(decisions) || decisions.length !== packages.length || decisions.some(x => typeof x !== "boolean")) {
    throw new Error("Incomplete SPDX policy results");
  }
  for (const [name, filter] of [["production", pkg => pkg.production], ["all", () => true]]) {
    const violations = packages.filter((pkg, i) => filter(pkg) && !decisions[i]);
    writeFileSync(`quality-output/npm-licenses-${name}.json`, JSON.stringify({ inventory: packages.filter(filter), violations }, null, 2) + "\n");
    if (violations.length) {
      console.error(`${name} license violations: ${JSON.stringify(violations)}`);
      process.exitCode = 1;
    }
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
