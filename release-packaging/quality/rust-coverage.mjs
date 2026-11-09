import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";

const reportPath = "quality-output/rust-lcov.info";
const logPath = "quality-output/rust-tests.log";
mkdirSync("quality-output", { recursive: true });
rmSync(reportPath, { force: true });
rmSync(logPath, { force: true });

function runCoverage(args) {
  const run = spawnSync("cargo", ["llvm-cov", ...args], {
    encoding: "utf8",
    maxBuffer: 100 * 1024 * 1024,
    shell: process.platform === "win32",
  });
  process.stdout.write(run.stdout ?? "");
  process.stderr.write(run.stderr ?? "");
  if (run.error) console.error(run.error);
  return run;
}

const output = ["--locked", "--lcov", "--output-path", reportPath];
const tests = runCoverage(["--workspace", "--all-features", "--no-fail-fast", ...output]);
const log = `${tests.stdout ?? ""}\n${tests.stderr ?? ""}`;
writeFileSync(logPath, log);

if (tests.error || tests.status !== 0) {
  process.exitCode = 1;
  // Only export after an observed test failure. A failed build can leave no fresh profiles.
  const failedTest = !tests.error && /^test result: FAILED\. \d+ passed; \d+ failed;/m.test(log);
  const report = failedTest ? runCoverage(["report", ...output]) : null;
  if (report?.status !== 0) rmSync(reportPath, { force: true });
}
