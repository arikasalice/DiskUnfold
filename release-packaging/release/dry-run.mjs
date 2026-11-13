// Local dry run of the release gate and packaging on the current OS (F15 WP1).
//
// It runs the same release-gate commands, product-resource generator, packagers and inventory
// builder as .github/workflows/release.yml, but never signs, attests, uploads or publishes, and
// every build it makes is a developer build. The report states what the real gate would decide.
//
// Besides the gate and packaging it runs the later stages on this OS's packages: the SBOM per
// package, the opened-package notices, license and link-list scan, the channel manifests and the fuzz gate
// (offline: it checks empty open-crash and Fuzz-run lists, because the dry run does not read GitHub).
// The workspace tests run with --run-tests; the workflow always runs them in every build cell.
//
// Usage: node release-packaging/release/dry-run.mjs --out <new directory> [--skip-web-build] [--run-tests]
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const exe = process.platform === "win32" ? ".exe" : "";
const steps = [];
const gateChecks = [];

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function run(command, args, options = {}) {
  console.log(`> ${command} ${args.join(" ")}`);
  const env = { ...process.env, ...(options.env ?? {}) };
  // An undefined value removes the variable from the child's environment.
  for (const [name, value] of Object.entries(env)) if (value === undefined) delete env[name];
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    shell: false,
    maxBuffer: 256 * 1024 * 1024,
    ...options,
    env,
  });
  if (result.error) throw result.error;
  return result;
}

function record(name, ok, detail = "") {
  steps.push({ name, ok, detail });
  console.log(`[${ok ? "pass" : "FAIL"}] ${name}${detail ? ": " + detail : ""}`);
}

/** Records a step that must fail; the gate would be broken if it accepted the input. */
function expectRejected(name, result) {
  const ok = result.status === 1;
  record(name, ok, ok ? (result.stderr ?? "").trim().split("\n")[0] : `exit ${result.status}, expected 1`);
}

/** Records one verdict of the real release gate; a failing verdict does not fail the dry run. */
function gateCheck(name, ok, detail = "") {
  gateChecks.push({ name, ok, detail });
  console.log(`[gate ${ok ? "pass" : "FAIL"}] ${name}${detail ? ": " + detail : ""}`);
}

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

/** Opens one package into `tree` the way the workflow does; returns an error text or "". */
function openPackage(row, file, tree) {
  mkdirSync(tree, { recursive: true });
  if (row.format === "tar-gz") return run("tar", ["-xzf", file, "-C", tree]).status === 0 ? "" : "tar failed";
  if (row.format === "deb") return run("dpkg-deb", ["-x", file, tree]).status === 0 ? "" : "dpkg-deb failed";
  if (row.format === "msix") {
    // The workflow uses unzip; Windows has no unzip, and its tar reads zip archives.
    const opened = process.platform === "win32" ? run("tar", ["-xf", file, "-C", tree]) : run("unzip", ["-q", file, "-d", tree]);
    return opened.status === 0 ? "" : "the MSIX could not be opened";
  }
  if (row.format !== "portable-exe") return `no way to open ${row.format} packages on this OS`;
  const data = mkdtempSync(path.join(os.tmpdir(), "du-scan-"));
  for (const [document, target] of [["LICENSE", "LICENSE"], ["THIRD-PARTY-NOTICES", "THIRD-PARTY-NOTICES.txt"]]) {
    const shown = spawnSync(file, ["notices", document], { env: { ...process.env, DISKUNFOLD_DATA_DIR: data } });
    if (shown.status !== 0) return `${row.file_name} does not embed ${document}`;
    writeFileSync(path.join(tree, target), shown.stdout);
  }
  return "";
}

/** SBOMs, opened-package scan, channel manifests and fuzz gate for this OS's packages. */
function laterStages(merged, stage, inventory) {
  const manifest = JSON.parse(readFileSync(merged, "utf8"));
  const sbomDir = path.join(outDir, "sbom");
  const sbom = run(tool("build-sbom"), [merged, root, sbomDir]);
  record("SBOM per package (CycloneDX, cargo and npm production dependencies)", sbom.status === 0, sbom.stderr.trim());
  if (sbom.status === 0) {
    const check = run(tool("release-gate"), ["check-sboms", "--manifest", merged, "--sbom-dir", sbomDir, "--notices", notices, "--repo", root]);
    record("SBOMs match the notices and the package digests", check.status === 0, check.stderr.trim());
  }
  const extracted = path.join(outDir, "extracted");
  const problems = manifest.rows.flatMap((row) => {
    const found = readdirSync(stage, { recursive: true }).filter((entry) => path.basename(entry) === row.file_name);
    return found.length === 1 ? [openPackage(row, path.join(stage, found[0]), path.join(extracted, row.file_name))].filter(Boolean) : [`${row.file_name} not found exactly once`];
  });
  record("packages opened for the scan", problems.length === 0, problems.join(" | "));
  const scan = run(tool("release-gate"), ["scan-artifacts", "--manifest", merged, "--extracted", extracted, "--staging", stage, "--license", path.join(root, "LICENSE"), "--notices", notices]);
  record("opened packages ship LICENSE and THIRD-PARTY-NOTICES byte for byte, hold no copyleft member, and link only system libraries", scan.status === 0, scan.stderr.trim());
  const channels = path.join(outDir, "channels");
  const made = run(tool("build-channel-manifests"), [path.join(inventory, "release-inventory.json"), channels, "--repository", "local/dry-run"]);
  record("channel manifests (direct, winget, store)", made.status === 0 && existsSync(path.join(channels, "channel-store.json")), made.stderr.trim());
}

/** The workflow packages with pwsh; Windows PowerShell 5.1 is the fallback. */
function powerShell() {
  return spawnSync("pwsh", ["-NoProfile", "-Command", "exit 0"]).status === 0 ? "pwsh" : "powershell.exe";
}

function currentScope() {
  const cpu = process.arch === "arm64" ? "arm64" : "x64";
  if (process.platform === "win32") return `windows:${cpu}`;
  if (process.platform === "linux") return `ubuntu:${cpu}`;
  if (process.platform === "darwin") return "macos:universal";
  throw new Error(`unsupported OS ${process.platform}`);
}

const out = argument("--out");
if (!out) {
  console.error("usage: dry-run.mjs --out <new directory> [--skip-web-build]");
  process.exit(2);
}
const outDir = path.resolve(out);
if (existsSync(outDir)) {
  console.error(`output directory already exists: ${outDir}`);
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });

const head = run("git", ["rev-parse", "HEAD"]).stdout.trim();
const dirty = run("git", ["status", "--porcelain", "--untracked-files=no"]).stdout.trim() !== "";
const version = /\[workspace\.package\][^[]*?\nversion = "([^"]+)"/.exec(readFileSync(path.join(root, "Cargo.toml"), "utf8"))?.[1];
const tag = `v${version}`;
const target = JSON.parse(run("cargo", ["metadata", "--no-deps", "--format-version", "1", "--locked"]).stdout).target_directory;
const tool = (name) => path.join(target, "debug", name + exe);

// 1. Gate tools.
const built = run("cargo", ["build", "--locked", "-p", "du-release-tools", "--bins"], { stdio: "inherit" });
record("build release tools", built.status === 0);
if (built.status !== 0) process.exit(1);

// 2. Gate: tag, commit, format versions, release-mode resources.
const gateArgs = (overrides = {}) => {
  const values = { tag, "workspace-version": version, "tag-commit": head, "checkout-commit": head, "event-commit": head, ...overrides };
  return ["check-tag", ...Object.entries(values).flatMap(([key, value]) => [`--${key}`, value])];
};
const tagCheck = run(tool("release-gate"), gateArgs());
let gatePassed = tagCheck.status === 0;
gateCheck("tag format, tag version, tag commit = checkout = event (simulated tag at HEAD)", tagCheck.status === 0, tagCheck.stderr.trim());
expectRejected("gate rejects a malformed tag", run(tool("release-gate"), gateArgs({ tag: "release-0.1" })));
expectRejected("gate rejects a tag that points elsewhere", run(tool("release-gate"), gateArgs({ "tag-commit": "0".repeat(40) })));
expectRejected("gate rejects a tag version that differs from the workspace", run(tool("release-gate"), gateArgs({ tag: "v9.9.9" })));

const formats = run(tool("release-gate"), ["read-formats", "--file", "release-packaging/release/format-versions.json"]);
gatePassed &&= formats.status === 0;
gateCheck("data format versions are set", formats.status === 0, formats.stderr.trim());

const releaseResources = path.join(outDir, "resources-release");
const release = run(tool("build-product-resources"), ["--release", "--source-commit", head, "--out", releaseResources]);
gatePassed &&= release.status === 0;
gateCheck("build-product-resources --release (notices, support table, every security basis confirmed)", release.status === 0, release.status === 0 ? "" : release.stderr.trim().split("\n").slice(0, 4).join(" | "));

const debianCopyright = path.join(root, "release-packaging", "linux", "debian-copyright");
const copyrightPresent = existsSync(debianCopyright) && statSync(debianCopyright).size > 0;
const copyrightReview = `${debianCopyright}.reviewed`;
const reviewRecord = existsSync(copyrightReview) ? readFileSync(copyrightReview, "utf8").split("\n") : [];
const copyrightDigest = copyrightPresent ? createHash("sha256").update(readFileSync(debianCopyright)).digest("hex") : "";
const copyrightReviewed = copyrightPresent && reviewRecord.includes(`sha256: ${copyrightDigest}`) && reviewRecord.some((line) => /^reviewer: \S/.test(line));
if (process.platform === "linux") {
  gatePassed &&= copyrightReviewed;
  gateCheck("reviewed Debian copyright file present", copyrightReviewed, copyrightReviewed ? "" : copyrightPresent ? "release-packaging/linux/debian-copyright.reviewed does not record the current sha256 and a reviewer" : "release-packaging/linux/debian-copyright is missing");
}
console.log(`\nThe real gate would ${gatePassed ? "PASS" : "FAIL"} on this tree. This dry run builds developer builds either way.\n`);

// 3. Developer resources and build.
const devResources = path.join(outDir, "resources");
const dev = run(tool("build-product-resources"), ["--allow-blockers", "--source-commit", head, "--out", devResources]);
record("developer product resources (blockers allowed)", dev.status === 0);
if (dev.status !== 0) process.exit(1);

if (!process.argv.includes("--skip-web-build")) {
  const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm";
  const web = [run(npmCommand, ["--prefix", "gui-web", "ci", "--ignore-scripts"], { stdio: "inherit", shell: process.platform === "win32" }),
    run(npmCommand, ["--prefix", "gui-web", "run", "build"], { stdio: "inherit", shell: process.platform === "win32" })];
  record("gui-web build", web.every((r) => r.status === 0));
}
const buildEnv = {
  DU_BUILD_KIND: "developer",
  DU_SOURCE_COMMIT: head,
  DU_BUILT_AT_UTC: new Date().toISOString().replace(/\.\d+Z$/, "Z"),
  DU_RELEASE_RESOURCES: path.relative(root, path.join(devResources, "product-resources.json")),
};
const cargoBuild = run("cargo", ["build", "--locked", "--release", "-p", "du-launcher"], { stdio: "inherit", env: buildEnv });
record("native launcher build (developer)", cargoBuild.status === 0);
if (cargoBuild.status !== 0) process.exit(1);
const binary = path.join(target, "release", "diskunfold" + exe);
const kind = run(process.execPath, ["release-packaging/release/check-build-kind.mjs", "--expected", "developer", "--commit", head, "--target-dir", target, "--binary", binary]);
record("embedded build kind is developer, commit matches, version output matches", kind.status === 0, (kind.stderr || kind.stdout).trim());
if (process.argv.includes("--run-tests")) {
  const tests = run("cargo", ["test", "--locked", "--workspace", "--all-features"], {
    stdio: "inherit",
    env: { DU_BUILD_KIND: undefined, DU_SOURCE_COMMIT: undefined, DU_RELEASE_TAG: undefined, DU_BUILT_AT_UTC: undefined, DU_APP_DB_FORMAT: undefined, DU_SNAPSHOT_FORMAT: undefined, DU_RELEASE_RESOURCES: undefined },
  });
  record("workspace tests (release.test_failed on failure)", tests.status === 0);
}

// 4. Package for this OS.
const stage = path.join(outDir, "stage");
mkdirSync(stage);
const notices = path.join(devResources, "THIRD-PARTY-NOTICES");
const parts = [];
if (process.platform === "win32") {
  const architecture = process.arch === "arm64" ? "arm64" : "x64";
  for (const format of ["portable-exe", "msix"]) {
    const name = `windows-${architecture}-${format}`;
    const result = run(powerShell(), ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(root, "release-packaging", "windows", "package-windows-development.ps1"),
      "-BinaryPath", binary, "-Version", version, "-Architecture", architecture, "-Format", format, "-OutputDirectory", path.join(stage, name),
      "-NoticesPath", notices,
      // Windows PowerShell 5.1 leaves $PSScriptRoot empty inside parameter defaults, so pass the icon.
      "-IconPath", path.join(root, "app-host", "icons", "icon.png")]);
    record(`package ${format}`, result.status === 0, result.status === 0 ? "" : (result.stderr || result.stdout).trim().split("\n").slice(-3).join(" | "));
    if (result.status === 0) parts.push(name);
  }
  // The MSI needs the pinned WiX (install-wix.ps1; DU_WIX_PATH) and Microsoft's WebView2 standalone
  // installer (fetch-webview2.ps1; DU_WEBVIEW2_INSTALLER). Without them the step fails and says why.
  const installer = process.env.DU_WEBVIEW2_INSTALLER;
  if (!installer || !process.env.DU_WIX_PATH) {
    record("package msi", false, "set DU_WIX_PATH (install-wix.ps1) and DU_WEBVIEW2_INSTALLER (fetch-webview2.ps1)");
  } else {
    const name = `windows-${architecture}-msi`;
    const result = run(powerShell(), ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(root, "release-packaging", "windows", "package-windows-msi.ps1"),
      "-BinaryPath", binary, "-Version", version, "-Architecture", architecture, "-NoticesPath", notices, "-WebView2InstallerPath", installer,
      "-IconPath", path.join(root, "app-host", "icons", "icon.ico"), "-OutputDirectory", path.join(stage, name)]);
    record("package msi", result.status === 0, result.status === 0 ? "" : (result.stderr || result.stdout).trim().split("\n").slice(-3).join(" | "));
    if (result.status === 0) parts.push(name);
  }
} else if (process.platform === "linux") {
  const cpu = process.arch === "arm64" ? "arm64" : "x64";
  const fixture = path.join(outDir, "debian-copyright.dry-run");
  writeFileSync(fixture, ["Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/", "Upstream-Name: DiskUnfold",
    "Upstream-Contact: DiskUnfold Maintainers <arikasalice@gmail.com>", "Source: https://github.com/arikasalice/DiskUnfold", "Files: *",
    "Copyright: DEVELOPER DRY RUN, incomplete", "License: MIT", ""].join("\n"));
  const result = run("bash", ["release-packaging/linux/package-linux.sh", "--binary", binary, "--version", version, "--cpu", cpu,
    "--notices", notices, "--debian-copyright", fixture, "--output-dir", path.join(stage, `linux-${cpu}`)]);
  record("package deb and tar.gz", result.status === 0, result.status === 0 ? "" : (result.stderr || result.stdout).trim().split("\n").slice(-3).join(" | "));
  if (result.status === 0) parts.push(`linux-${cpu}`);
} else {
  record("package dmg", false, "macOS dry run needs both slices and lipo; run the macOS steps of the workflow instead");
}

// 5. Merge, check, inventory.
if (parts.length > 0) {
  const merged = path.join(outDir, "merged-manifest.json");
  const merge = run(tool("release-gate"), ["merge-manifests", "--out", merged, ...parts.map((name) => `${name}=${path.join(stage, name)}`)]);
  record("merge platform manifests", merge.status === 0, merge.stderr.trim());
  const set = run(tool("release-gate"), ["check-release-set", "--manifest", merged, "--signing-applied", "false", "--scope", currentScope()]);
  record(`release set complete for ${currentScope()} and no unproven signature claims`, set.status === 0, (set.stderr || "").trim().split("\n").join(" | "));
  const inventory = path.join(outDir, "inventory");
  const fuzzRecord = path.join(outDir, "fuzz-gate.json");
  writeFileSync(path.join(outDir, "open-fuzz-crashes.json"), "[]\n");
  writeFileSync(path.join(outDir, "fuzz-runs.json"), "[]\n");
  const fuzz = run(tool("release-gate"), ["check-fuzz", "--issues", path.join(outDir, "open-fuzz-crashes.json"), "--runs", path.join(outDir, "fuzz-runs.json"),
    "--registry", path.join(root, "release-packaging", "quality", "fuzz-targets.json"), "--now", String(Math.floor(Date.now() / 1000)), "--commit", head,
    "--require-run", "false", "--record-out", fuzzRecord]);
  record("fuzz gate (offline: no open crash and no Fuzz run list is read from GitHub)", fuzz.status === 0, (fuzz.stdout.match(/::warning::.*/g) ?? []).join(" | "));
  const inv = run(tool("build-release-inventory"), [merged, stage, inventory, "--fuzz-gate", fuzzRecord]);
  record("release inventory, SHA256SUMS, EXECUTABLE-SHA256.json, DISTRIBUTION.md", inv.status === 0, inv.stderr.trim());
  if (inv.status === 0) {
    let mismatches = 0;
    const lines = readFileSync(path.join(inventory, "SHA256SUMS"), "utf8").split("\n").filter(Boolean);
    for (const line of lines) {
      const [digest, name] = [line.slice(0, 64), line.slice(66)];
      const matches = readdirSync(stage, { recursive: true }).filter((entry) => path.basename(entry) === name);
      if (matches.length !== 1 || sha256(path.join(stage, matches[0])) !== digest) mismatches += 1;
    }
    record("SHA256SUMS rechecked against the staged package bytes", lines.length > 0 && mismatches === 0, `${lines.length} packages`);
    console.log("\n" + readFileSync(path.join(inventory, "DISTRIBUTION.md"), "utf8"));
    laterStages(merged, stage, inventory);
  }
}

console.log("Skipped by design: signing (SignPath, Apple), SLSA attestation, provenance verification and the draft release exist only in CI.");
const report = { head, dirty, tag, version, platform: `${process.platform}-${process.arch}`, official: false, realGateWouldPass: gatePassed, gateChecks, steps };
writeFileSync(path.join(outDir, "dry-run-report.json"), JSON.stringify(report, null, 2) + "\n");
const failed = steps.filter((step) => !step.ok);
console.log(`\nDry run finished: ${steps.length - failed.length}/${steps.length} steps passed; official build: no; real gate would pass: ${gatePassed ? "yes" : "no"}.`);
process.exit(failed.length === 0 ? 0 : 1);
