// Static policy lint for .github/workflows/release.yml (F15 release pipeline).
// It does not replace actionlint; it enforces repository rules that actionlint cannot know:
// every action is pinned to a commit SHA with a version comment, no unconfirmed SHA marker
// remains, the triggers are the release tag push and manual dry runs only, official builds
// cannot be hard-coded, a release can only be created as a draft, write permissions and the
// OIDC token stay in the one job that needs them, jobs after the gate run only for official
// builds, secrets never reach job-level env, and run scripts take no workflow expressions
// other than matrix values.
// Usage: node lint-workflow.mjs [workflow-file] [--verify-remote]
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const USES = /^\s*(?:-\s+)?uses:\s*(\S+)\s*(?:#\s*(\S+))?\s*$/;
const PINNED = /^([\w.-]+\/[\w.-]+(?:\/[\w./-]+)?)@([0-9a-f]{40})$/;

/** Splits the jobs: section into { name, lines } blocks by two-space indented keys. */
export function jobBlocks(text) {
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((line) => /^jobs:\s*$/.test(line));
  if (start < 0) return [];
  const blocks = [];
  for (const line of lines.slice(start + 1)) {
    if (/^\S/.test(line)) break;
    const header = /^  ([\w-]+):\s*$/.exec(line);
    if (header) blocks.push({ name: header[1], lines: [] });
    else if (blocks.length) blocks.at(-1).lines.push(line);
  }
  return blocks;
}

function jobPermissions(block) {
  const found = new Map();
  const index = block.lines.findIndex((line) => /^    permissions:\s*$/.test(line));
  if (index < 0) return found;
  for (const line of block.lines.slice(index + 1)) {
    const entry = /^      ([\w-]+):\s*(\w+)\s*$/.exec(line);
    if (!entry) break;
    found.set(entry[1], entry[2]);
  }
  return found;
}

/** Policy problems in permissions, job conditions, secrets and run scripts. */
export function lintJobs(text) {
  const problems = [];
  const blocks = jobBlocks(text);
  for (const block of blocks) {
    for (const [scope, level] of jobPermissions(block)) {
      if (scope === "id-token" && level !== "none" && block.name !== "attest") {
        problems.push(`permissions: job ${block.name} must not request id-token (only attest may)`);
      }
      if (scope === "attestations" && level === "write" && block.name !== "attest") {
        problems.push(`permissions: job ${block.name} must not write attestations (only attest may)`);
      }
      if (scope === "contents" && level === "write" && block.name !== "publish-draft") {
        problems.push(`permissions: job ${block.name} must not write contents (only publish-draft may)`);
      }
      if (!["contents", "id-token", "attestations"].includes(scope) && level === "write") {
        problems.push(`permissions: job ${block.name} requests unexpected write access to ${scope}`);
      }
    }
    const jobEnv = block.lines.findIndex((line) => /^    env:\s*$/.test(line));
    if (jobEnv >= 0) {
      for (const line of block.lines.slice(jobEnv + 1)) {
        if (!/^      \S/.test(line)) break;
        if (/\$\{\{\s*secrets\./.test(line)) problems.push(`secrets: job ${block.name} puts a secret in job-level env; scope it to the one step`);
      }
    }
  }
  for (const name of ["attest", "verify-provenance", "publish-draft"]) {
    const block = blocks.find((candidate) => candidate.name === name);
    if (!block) {
      problems.push(`jobs: ${name} is missing`);
    } else if (!block.lines.some((line) => /^    if:\s*needs\.gate\.outputs\.official == 'true'\s*$/.test(line))) {
      problems.push(`jobs: ${name} must run only when the gate output official is 'true'`);
    }
  }
  if (!blocks.find((block) => block.name === "attest") || !jobPermissions(blocks.find((block) => block.name === "attest")).get("id-token")) {
    problems.push("permissions: attest must hold id-token: write");
  }
  const lines = text.split(/\r?\n/);
  lines.forEach((line, index) => {
    const run = /^(\s*)(?:-\s+)?run:\s*(.*)$/.exec(line);
    if (!run) return;
    const body = [run[2]];
    if (/^[|>][+-]?$/.test(run[2])) {
      body.length = 0;
      const baseIndent = run[1].length + (line.trimStart().startsWith("-") ? 2 : 0);
      for (const next of lines.slice(index + 1)) {
        if (next.trim() !== "" && next.search(/\S/) <= baseIndent) break;
        body.push(next);
      }
    }
    for (const expression of body.join("\n").matchAll(/\$\{\{\s*([^}]*?)\s*\}\}/g)) {
      if (!/^matrix\.[\w-]+$/.test(expression[1])) {
        problems.push(`line ${index + 1}: run script embeds ${expression[0]}; pass it through env instead`);
      }
    }
  });
  return problems;
}

/** The single place that may set official=true is the tag push branch of the gate. */
export function lintGateMode(text) {
  const problems = [];
  const lines = text.split(/\r?\n/);
  const official = lines.flatMap((line, index) => (/echo "official=true"/.test(line) ? [index] : []));
  const branch = lines.findIndex((line) => /if \[\[ "\$EVENT_NAME" == push && "\$REF_TYPE" == tag \]\]; then/.test(line));
  const other = lines.findIndex((line, index) => index > branch && /^\s*else\s*$/.test(line));
  const check = lines.findIndex((line) => /release-gate check-tag/.test(line));
  if (branch < 0 || other < 0 || official.length !== 1 || !(branch < check && check < official[0] && official[0] < other)) {
    problems.push("gate: official=true must be written once, in the push tag branch and after release-gate check-tag");
  }
  return problems;
}

/** What each release stage job must run (F15 HLD table 7 stages 3 to 9). */
const STAGE_JOBS = {
  "artifact-scan": ["release-gate scan-artifacts"],
  "notices-sbom": ["build-sbom", "release-gate check-sboms"],
  "release-gate": ["release-gate check-fuzz", "--label fuzz-crash"],
  "channel-manifests": ["build-channel-manifests"],
};

/**
 * The stage jobs must exist, run their tool, run for dry runs too (no job-level condition), and be
 * wired into the jobs that depend on them; the build cells must run the workspace tests and name
 * release.test_failed.
 */
export function lintStages(text) {
  const problems = [];
  const blocks = jobBlocks(text);
  const find = (name) => blocks.find((block) => block.name === name);
  const needs = (block) => block?.lines.find((line) => /^    needs:/.test(line)) ?? "";
  for (const [name, commands] of Object.entries(STAGE_JOBS)) {
    const block = find(name);
    if (!block) {
      problems.push(`stages: job ${name} is missing`);
      continue;
    }
    if (block.lines.some((line) => /^    if:/.test(line))) {
      problems.push(`stages: job ${name} must run for dry runs too, so it takes no job-level if`);
    }
    for (const command of commands) {
      if (!block.lines.some((line) => line.includes(command))) problems.push(`stages: job ${name} must run ${command}`);
    }
  }
  const build = find("build")?.lines.join("\n") ?? "";
  if (!/cargo test --locked --workspace --all-features/.test(build) || !build.includes("release.test_failed")) {
    problems.push("stages: build must run cargo test --locked --workspace --all-features and report release.test_failed");
  }
  for (const [job, required] of [["inventory", ["artifact-scan", "notices-sbom"]], ["attest", ["release-gate"]], ["publish-draft", ["release-gate"]]]) {
    for (const name of required) {
      if (!new RegExp(`[\\[ ,]${name}[\\], ]`).test(needs(find(job)))) problems.push(`stages: job ${job} must need ${name}`);
    }
  }
  return problems;
}

/** Returns the list of policy problems found in the workflow text. */
export function lintWorkflow(text) {
  const problems = [];
  const lines = text.split(/\r?\n/);
  lines.forEach((line, index) => {
    const where = `line ${index + 1}`;
    if (/TODO/.test(line)) {
      problems.push(`${where}: unresolved TODO (an unconfirmed action SHA must fail the lint)`);
    }
    const match = USES.exec(line);
    if (match && !match[1].startsWith("./")) {
      const pinned = PINNED.exec(match[1]);
      if (!pinned) {
        problems.push(`${where}: action ${match[1]} is not pinned to a 40 digit commit SHA`);
      } else if (!match[2] || !/^v\d/.test(match[2])) {
        problems.push(`${where}: pinned action ${pinned[1]} needs a "# vX.Y.Z" version comment`);
      }
    } else if (/^\s*(?:-\s+)?uses:/.test(line) && !match) {
      problems.push(`${where}: cannot read this uses: line`);
    }
    if (/DU_BUILD_KIND:\s*["']?official/.test(line)) {
      problems.push(`${where}: DU_BUILD_KIND must come from the gate output, never a literal`);
    }
    if (/gh release create/.test(line) === false && /gh release (edit|upload)/.test(line)) {
      problems.push(`${where}: only gate-checked draft creation may touch releases`);
    }
  });
  const trigger = text.match(/^on:\n((?:[ \t]+.*\n|\n)+)/m)?.[1] ?? "";
  if (!/^\s+push:\n\s+tags:/m.test(trigger)) problems.push("trigger: push must be limited to tags");
  if (!/^\s+workflow_dispatch:/m.test(trigger)) problems.push("trigger: workflow_dispatch dry run is missing");
  for (const forbidden of ["pull_request", "pull_request_target", "schedule", "branches:", "release:", "workflow_run"]) {
    if (trigger.includes(forbidden)) problems.push(`trigger: ${forbidden} is not allowed`);
  }
  if (!/^permissions:\n\s+contents: read\s*$/m.test(text)) {
    problems.push("permissions: the workflow default must be contents: read");
  }
  const create = text.split("\n").filter((line) => /gh release create/.test(line));
  const createBlock = create.length ? text.slice(text.indexOf(create[0])) : "";
  if (create.length !== 1 || !/--draft/.test(createBlock.split("release-assets/*")[0])) {
    problems.push("publish: exactly one gh release create is allowed and it must use --draft");
  }
  if (/--draft=false|--latest|gh release edit/.test(text)) {
    problems.push("publish: the workflow must never publish a release");
  }
  if (!/attest-build-provenance@/.test(text)) problems.push("provenance: attest-build-provenance step is missing");
  return [...problems, ...lintJobs(text), ...lintGateMode(text), ...lintStages(text)];
}

/** Lists the distinct pinned actions as { repository, sha, version }. */
export function pinnedActions(text) {
  const seen = new Map();
  for (const line of text.split(/\r?\n/)) {
    const match = USES.exec(line);
    const pinned = match && PINNED.exec(match[1]);
    if (pinned) seen.set(`${pinned[1]}@${pinned[2]}`, { repository: pinned[1].split("/").slice(0, 2).join("/"), sha: pinned[2], version: match[2] });
  }
  return [...seen.values()];
}

/** Checks with git ls-remote that each version comment resolves to the pinned commit. */
export function verifyRemote(actions, run = execFileSync) {
  const problems = [];
  for (const action of actions) {
    const output = run(
      "git",
      ["ls-remote", `https://github.com/${action.repository}`, `refs/tags/${action.version}`, `refs/tags/${action.version}^{}`],
      { encoding: "utf8" },
    );
    const shas = output.split("\n").filter(Boolean).map((line) => line.split("\t")[0]);
    if (!shas.includes(action.sha)) {
      problems.push(`${action.repository}: tag ${action.version} does not resolve to ${action.sha} (found ${shas.join(", ") || "nothing"})`);
    }
  }
  return problems;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const file = process.argv.slice(2).find((arg) => !arg.startsWith("--"))
    ?? path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..", ".github", "workflows", "release.yml");
  const text = readFileSync(file, "utf8");
  const problems = lintWorkflow(text);
  if (process.argv.includes("--verify-remote")) problems.push(...verifyRemote(pinnedActions(text)));
  if (problems.length > 0) {
    console.error(`release workflow lint failed (${file}):\n  ${problems.join("\n  ")}`);
    process.exit(1);
  }
  console.log(`release workflow lint passed: ${pinnedActions(text).length} pinned actions`);
}
