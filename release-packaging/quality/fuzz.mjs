import { execFileSync, spawnSync } from "node:child_process";
import {
  copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync,
} from "node:fs";
import { isAbsolute, join, relative, sep } from "node:path";

function inside(parent, child) {
  const path = relative(parent, child);
  return path !== "" && path !== ".." && !path.startsWith(`..${sep}`) && !isAbsolute(path);
}

function requireFile(root, file) {
  const path = join(root, file);
  if (!existsSync(path) || !inside(root, realpathSync(path)) || !statSync(path).isFile()) {
    throw new Error(`Required fuzz file is missing or outside its crate: ${file}`);
  }
}

function validateRegistry(root, registry) {
  if (!Array.isArray(registry?.targets)) throw new Error("Invalid fuzz target registry");
  if (registry.targets.length === 0) return [];
  if (!/^nightly-\d{4}-\d{2}-\d{2}$/.test(registry.toolchain) ||
      !/^\d+\.\d+\.\d+$/.test(registry.cargoFuzzVersion)) {
    throw new Error("Pin a dated nightly and an exact cargo-fuzz version");
  }

  const seen = new Set();
  return registry.targets.map(({ owner, name, seconds }) => {
    if (typeof owner !== "string" || !/^[a-z][a-z0-9-]*(\/[a-z][a-z0-9-]*)*$/.test(owner) ||
        typeof name !== "string" || !/^[a-zA-Z0-9_][a-zA-Z0-9_-]*$/.test(name) ||
        !Number.isInteger(seconds) || seconds < 1 || seconds > 600) {
      throw new Error("A fuzz target requires a crate path, a target name and 1–600 seconds");
    }
    const id = `${owner}/${name}`;
    if (seen.has(id)) throw new Error(`Duplicate fuzz target: ${id}`);
    seen.add(id);

    const crate = join(root, owner);
    if (!existsSync(crate) || !inside(root, realpathSync(crate))) {
      throw new Error(`Invalid fuzz owner: ${owner}`);
    }
    requireFile(crate, "Cargo.toml");
    requireFile(crate, "fuzz/Cargo.toml");
    requireFile(crate, "fuzz/Cargo.lock");
    return { owner, name, seconds, crate };
  });
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: "inherit", ...options });
  if (result.error || result.status !== 0) {
    throw new Error(result.error?.message ?? `${command} exited with ${result.status ?? result.signal}`);
  }
}

function checkTarget({ crate, owner, name }) {
  const manifest = realpathSync(join(crate, "fuzz", "Cargo.toml"));
  const result = execFileSync(
    "cargo",
    ["metadata", "--locked", "--no-deps", "--format-version", "1", "--manifest-path", manifest],
    { cwd: crate, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
  );
  const { packages } = JSON.parse(result);
  const pkg = packages.find(({ manifest_path }) => realpathSync(manifest_path) === manifest);
  const binary = pkg?.targets.find((target) => target.name === name && target.kind.includes("bin"));
  if (!binary || !existsSync(binary.src_path) || !inside(crate, realpathSync(binary.src_path))) {
    throw new Error(`Fuzz target is not declared in its owner crate: ${owner}/${name}`);
  }
}

function runTarget(target, toolchain, output) {
  const { owner, name, seconds, crate } = target;
  const lockPath = join(crate, "fuzz", "Cargo.lock");
  const lock = readFileSync(lockPath);
  const corpus = join(output, "fuzz-corpus", owner, name);
  const crashes = join(output, "fuzz-crashes", owner, name);
  const artifacts = join(crate, "fuzz", "artifacts", name);
  const seeds = join(crate, "fuzz", "corpus", name);
  mkdirSync(corpus, { recursive: true });

  try {
    run("cargo", [`+${toolchain}`, "fetch", "--locked", "--manifest-path", "fuzz/Cargo.toml"], {
      cwd: crate,
    });
    run(
      "cargo",
      [
        `+${toolchain}`, "fuzz", "run", name, corpus,
        ...(existsSync(seeds) ? [seeds] : []),
        "--", `-max_total_time=${seconds}`, `-timeout=${seconds}`,
      ],
      { cwd: crate, env: { ...process.env, RUSTUP_TOOLCHAIN: toolchain, CARGO_NET_OFFLINE: "true" } },
    );
  } finally {
    if (existsSync(artifacts) && inside(crate, realpathSync(artifacts))) {
      for (const file of readdirSync(artifacts, { withFileTypes: true })) {
        if (!file.isFile()) continue;
        mkdirSync(crashes, { recursive: true });
        copyFileSync(join(artifacts, file.name), join(crashes, file.name));
      }
    }
    if (!readFileSync(lockPath).equals(lock)) throw new Error(`Fuzz lockfile changed: ${owner}`);
  }
}

function main() {
  const root = realpathSync(process.cwd());
  const output = join(root, "quality-output");
  const report = { status: "failed", results: [] };
  try {
    const registry = JSON.parse(readFileSync(join(root, "release-packaging/quality/fuzz-targets.json"), "utf8"));
    const targets = validateRegistry(root, registry);
    for (const target of targets) checkTarget(target);
    if (targets.length === 0) {
      report.status = "not configured";
    } else if (process.argv.includes("--check")) {
      report.status = "validated";
      report.results = targets.map(({ owner, name, seconds }) => ({ owner, name, seconds, status: "validated" }));
    } else {
      const { toolchain, cargoFuzzVersion } = registry;
      run("rustup", ["toolchain", "install", toolchain, "--profile", "minimal"]);
      run("cargo", [`+${toolchain}`, "install", "--locked", "cargo-fuzz", "--version", cargoFuzzVersion]);
      for (const target of targets) {
        const { owner, name, seconds } = target;
        try {
          runTarget(target, toolchain, output);
          report.results.push({ owner, name, seconds, status: "passed" });
        } catch (error) {
          report.results.push({ owner, name, seconds, status: "failed", error: error.message });
        }
      }
      if (report.results.every(({ status }) => status === "passed")) report.status = "passed";
    }
  } catch (error) {
    report.error = error.message;
  }
  mkdirSync(output, { recursive: true });
  writeFileSync(join(output, "fuzz-report.json"), JSON.stringify(report, null, 2) + "\n");
  console.log(JSON.stringify(report, null, 2));
  if (report.status === "failed") process.exitCode = 1;
}

main();
