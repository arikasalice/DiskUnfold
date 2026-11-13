// Static policy lint for .github/workflows/release.yml (F15 release pipeline).
// It does not replace actionlint; it enforces repository rules that actionlint cannot know:
// every action is pinned to a commit SHA with a version comment, no unconfirmed SHA marker
// remains, the triggers are the release tag push and manual dry runs only, official builds
// cannot be hard-coded, and a release can only be created as a draft.
// Usage: node lint-workflow.mjs [workflow-file] [--verify-remote]
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const USES = /^\s*(?:-\s+)?uses:\s*(\S+)\s*(?:#\s*(\S+))?\s*$/;
const PINNED = /^([\w.-]+\/[\w.-]+(?:\/[\w./-]+)?)@([0-9a-f]{40})$/;

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
  return problems;
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
