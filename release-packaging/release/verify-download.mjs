// Verifies one downloaded DiskUnfold release file (F15 FR2.HD1, VERIFY.md).
//
//   node verify-download.mjs <file> --repo <owner/repo> --tag <vX.Y.Z> [--sums SHA256SUMS]
//        [--provenance release-provenance.json] [--verifier <verify-release-provenance>]
//        [--gh <absolute gh path>] [--gh-sha256 <hex>]
//
// Result, matching the three outcomes of the published guidance:
//   verified     exit 0  checksum matches (when SHA256SUMS is given) and GitHub attestation verified
//   mismatch     exit 1  checksum or attestation check failed; do not use the file
//   unverifiable exit 3  the check could not run (no gh, no network); the file is NOT verified
// Exit 2 is a usage error. Showing a digest is never reported as verification.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const OFFLINE = /dial tcp|timeout|timed out|no such host|could not resolve|network is unreachable|connection refused|TLS handshake|EOF/i;

export function sha256File(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

/** Looks up the file in a SHA256SUMS text ("<digest>  <name>"). */
export function expectedDigest(sums, name) {
  for (const line of sums.split(/\r?\n/)) {
    const match = /^([0-9a-f]{64}) [ *](.+)$/.exec(line);
    if (match && match[2] === name) return match[1];
  }
  return undefined;
}

/** Classifies a finished gh attestation run as "verified", "mismatch" or "unverifiable". */
export function classifyAttestation(status, stderr) {
  if (status === 0) return "verified";
  if (status === null || OFFLINE.test(stderr)) return "unverifiable";
  return "mismatch";
}

/** The release workflow identity a download must come from; builder id is the signer workflow ref. */
export function expectedPolicy(repo, tag) {
  return {
    workflowPath: ".github/workflows/release.yml",
    eventName: "push",
    builderId: `https://github.com/${repo}/.github/workflows/release.yml@refs/tags/${tag}`,
  };
}

function argument(args, name) {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : undefined;
}

function findGh(given) {
  if (given) return path.resolve(given);
  const finder = process.platform === "win32" ? "where.exe" : "which";
  const found = spawnSync(finder, ["gh"], { encoding: "utf8" });
  const first = found.status === 0 ? found.stdout.split(/\r?\n/).find(Boolean) : undefined;
  return first ? realpathSync(first) : undefined;
}

function finish(result, lines) {
  console.log(lines.join("\n"));
  console.log(`RESULT: ${result}`);
  process.exit({ verified: 0, mismatch: 1, unverifiable: 3 }[result]);
}

export function main(args) {
  const file = args.find((arg, index) => !arg.startsWith("--") && !(index > 0 && args[index - 1].startsWith("--")));
  const repo = argument(args, "--repo");
  const tag = argument(args, "--tag");
  if (!file || !/^[\w.-]+\/[\w.-]+$/.test(repo ?? "") || !/^v\d+\.\d+\.\d+$/.test(tag ?? "")) {
    console.error("usage: verify-download.mjs <file> --repo <owner/repo> --tag <vX.Y.Z> [--sums F] [--provenance F] [--verifier BIN] [--gh PATH] [--gh-sha256 HEX]");
    process.exit(2);
  }
  const notes = [];
  const digest = sha256File(file);
  notes.push(`file: ${file}\nsha256: ${digest}`);

  const sumsFile = argument(args, "--sums");
  if (sumsFile) {
    const expected = expectedDigest(readFileSync(sumsFile, "utf8"), path.basename(file));
    if (!expected) finish("unverifiable", [...notes, `${path.basename(file)} is not listed in ${sumsFile}. Download SHA256SUMS from the same release page.`]);
    if (expected !== digest) finish("mismatch", [...notes, `SHA256SUMS lists ${expected}. Stop using this file and download it again from the official release page.`]);
    notes.push("checksum: matches SHA256SUMS");
  } else {
    notes.push("checksum: not compared (no --sums); pass SHA256SUMS from the release page");
  }

  const gh = findGh(argument(args, "--gh"));
  if (!gh) finish("unverifiable", [...notes, "GitHub CLI (gh 2.102.0 or newer) was not found. Install it from https://cli.github.com and run this again."]);
  const ghDigest = sha256File(gh);
  const pinned = argument(args, "--gh-sha256");
  if (pinned && pinned !== ghDigest) finish("mismatch", [...notes, `gh at ${gh} has SHA-256 ${ghDigest}, not the pinned ${pinned}.`]);
  notes.push(`gh: ${gh} (sha256 ${ghDigest}${pinned ? ", matches --gh-sha256" : "; taken from this machine, so it is not an independent pin"})`);

  const provenanceFile = argument(args, "--provenance");
  const verifier = argument(args, "--verifier");
  let run;
  if (verifier) {
    if (!provenanceFile) finish("unverifiable", [...notes, "--verifier needs --provenance release-provenance.json from the release page."]);
    const provenance = JSON.parse(readFileSync(provenanceFile, "utf8"));
    if (provenance.official !== true || provenance.repository !== repo || provenance.tag !== tag) {
      finish("mismatch", [...notes, `release-provenance.json does not describe an official ${repo} ${tag} release.`]);
    }
    // The file comes from the same release page as the package, so it is not a trust root: the
    // workflow identity is fixed here and the file may only repeat it.
    const policy = expectedPolicy(repo, tag);
    if (provenance.workflowPath !== policy.workflowPath || provenance.eventName !== policy.eventName || provenance.builderId !== policy.builderId) {
      finish("mismatch", [...notes, "release-provenance.json names a workflow, event or builder other than this repository's release workflow at the tag."]);
    }
    run = spawnSync(path.resolve(verifier), [path.resolve(file), repo, tag, provenance.commit, provenance.runId, provenance.runAttempt,
      policy.workflowPath, policy.eventName, policy.builderId, gh, ghDigest, "--deny-self-hosted-runners"], { encoding: "utf8" });
  } else {
    run = spawnSync(gh, ["attestation", "verify", path.resolve(file), "--hostname", "github.com", "--repo", repo,
      "--signer-workflow", `${repo}/.github/workflows/release.yml`, "--source-ref", `refs/tags/${tag}`, "--deny-self-hosted-runners"], { encoding: "utf8" });
  }
  if (run.error) finish("unverifiable", [...notes, `could not start the verifier: ${run.error.message}`]);
  const result = classifyAttestation(run.status, `${run.stderr}${run.stdout}`);
  const next = {
    verified: "The file's SHA-256 is covered by a GitHub build attestation from this repository's release workflow at the tag.",
    mismatch: "No matching attestation was accepted. Stop using the file and download it again from the official release page.",
    unverifiable: "The check could not reach GitHub. The file is not verified; retry with a network connection.",
  }[result];
  finish(result, [...notes, (run.stdout + run.stderr).trim().split("\n").slice(-6).join("\n"), next]);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2));
