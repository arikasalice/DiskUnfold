import { execFileSync } from 'node:child_process';
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export const HOSTED_RUNNERS = Object.freeze({
  'linux-x64': 'ubuntu-24.04',
  'linux-arm64': 'ubuntu-24.04-arm',
  'windows-x64': 'windows-2022',
  'windows-arm64': 'windows-11-arm',
  'macos-x64': 'macos-15-intel',
  'macos-arm64': 'macos-15',
});

/**
 * Targets that default to self-hosted runners in `auto` mode because their GitHub-hosted runners
 * (`macos-15-intel` ~16m, `windows-11-arm` ~10m, `windows-2022` ~8m) are the slowest bottlenecks
 * in CI, whereas `ubuntu-24.04`, `ubuntu-24.04-arm`, and `macos-15` complete fast on dedicated
 * GitHub-hosted VMs without contending for a single local host machine's CPU and I/O.
 */
export const AUTO_SELF_HOSTED_TARGETS = Object.freeze(
  new Set(['macos-x64', 'windows-x64', 'windows-arm64']),
);

export const SELF_HOSTED_RUNNERS = Object.freeze({
  'linux-x64': Object.freeze(['self-hosted', 'Linux', 'X64']),
  'linux-arm64': Object.freeze(['self-hosted', 'Linux', 'ARM64']),
  'windows-x64': Object.freeze(['self-hosted', 'Windows', 'X64']),
  'windows-arm64': Object.freeze(['self-hosted', 'Windows', 'ARM64']),
  'macos-x64': Object.freeze(['self-hosted', 'macOS', 'X64']),
  'macos-arm64': Object.freeze(['self-hosted', 'macOS', 'ARM64']),
});

function sameLabels(actual, expected) {
  return (
    Array.isArray(actual) &&
    actual.length === expected.length &&
    actual.every((label, index) => label === expected[index])
  );
}

function hasOnlineSelfHostedRunner(onlineRunners, requiredLabels, faultyRunnerNames) {
  if (!Array.isArray(onlineRunners)) return false;
  const required = requiredLabels.map((label) => String(label).toLowerCase());
  return onlineRunners.some((runner) => {
    if (!runner || runner.status !== 'online' || !Array.isArray(runner.labels)) return false;
    if (runner.name && faultyRunnerNames.has(runner.name)) return false;
    const present = new Set(
      runner.labels
        .map((entry) => (typeof entry === 'string' ? entry : entry?.name))
        .filter(Boolean)
        .map((name) => String(name).toLowerCase()),
    );
    return required.every((label) => present.has(label));
  });
}

/**
 * Queries the GitHub Actions runners API in real time for the given repository.
 * Returns the runner list on success, or null when the API cannot be reached.
 */
export function queryOnlineRunners({
  repository = process.env.GITHUB_REPOSITORY,
  token = process.env.RUNNER_STATUS_TOKEN || process.env.GH_TOKEN,
  run = execFileSync,
} = {}) {
  if (!repository) return null;
  try {
    const env = token ? { ...process.env, GH_TOKEN: token } : process.env;
    const raw = run('gh', ['api', `/repos/${repository}/actions/runners`, '--paginate'], {
      encoding: 'utf8',
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed?.runners) ? parsed.runners : null;
  } catch {
    return null;
  }
}

/**
 * Inspects the most recent quality.yml runs for self-hosted runners whose latest completed job
 * failed during "Set up job" (for example, broken tar/QEMU extraction or container initialization
 * before any workflow step runs). If the runner later completed "Set up job" successfully, it is
 * considered healthy again.
 */
export function queryFaultyRunners({
  repository = process.env.GITHUB_REPOSITORY,
  token = process.env.RUNNER_STATUS_TOKEN || process.env.GH_TOKEN,
  currentRunId = process.env.GITHUB_RUN_ID,
  run = execFileSync,
} = {}) {
  if (!repository) return [];
  try {
    const env = token ? { ...process.env, GH_TOKEN: token } : process.env;
    const runsRaw = run(
      'gh',
      ['api', `/repos/${repository}/actions/workflows/quality.yml/runs?per_page=3`],
      { encoding: 'utf8', env, stdio: ['ignore', 'pipe', 'pipe'] },
    );
    const workflowRuns = JSON.parse(runsRaw)?.workflow_runs ?? [];
    for (const wfRun of workflowRuns) {
      if (!wfRun?.id || String(wfRun.id) === String(currentRunId)) continue;
      if (wfRun.status !== 'completed') continue;
      const jobsRaw = run(
        'gh',
        ['api', `/repos/${repository}/actions/runs/${wfRun.id}/jobs?per_page=30`],
        { encoding: 'utf8', env, stdio: ['ignore', 'pipe', 'pipe'] },
      );
      const jobs = JSON.parse(jobsRaw)?.jobs ?? [];
      const faulty = new Set();
      for (const job of jobs) {
        const runnerName = job?.runner_name;
        if (!runnerName || job.status !== 'completed') continue;
        const setupStep = (job.steps ?? []).find((step) => step.name === 'Set up job');
        if (setupStep?.conclusion === 'failure') {
          faulty.add(runnerName);
        }
      }
      return [...faulty];
    }
    return [];
  } catch {
    return [];
  }
}

/**
 * Validates the six-target manifest and resolves each target's active runner in real time.
 * Selects the target's self-hosted runner only when the live runner status confirms an online,
 * non-faulty runner with matching labels, and automatically falls back to the GitHub-hosted runner
 * when the self-hosted runner is offline, faulty, unregistered, or unreachable.
 */
export function buildManifest(manifest, options = {}) {
  if (!manifest || !Array.isArray(manifest.targets)) {
    throw new Error('Expected manifest.targets array');
  }
  const runnerMode = (options.runnerMode ?? process.env.DU_RUNNER_MODE ?? 'auto')
    .trim()
    .toLowerCase();
  if (!['auto', 'github-hosted', 'self-hosted-all'].includes(runnerMode)) {
    throw new Error(`Invalid runner mode: ${runnerMode}`);
  }
  const onlineRunners = options.onlineRunners ?? null;
  const faultyRunnerNames = new Set(options.faultyRunners ?? []);
  const preferredTargets = options.preferredTargets ?? AUTO_SELF_HOSTED_TARGETS;

  const keys = new Set();
  const resolvedTargets = [];
  for (const target of manifest.targets) {
    const key = `${target.os}-${target.cpu}`;
    const expectedHosted = HOSTED_RUNNERS[key];
    const expectedSelfHosted = SELF_HOSTED_RUNNERS[key];
    if (
      keys.has(key) ||
      !expectedHosted ||
      target.runner !== expectedHosted ||
      !target.rustTarget ||
      (target.selfHostedRunner !== undefined &&
        !sameLabels(target.selfHostedRunner, expectedSelfHosted))
    ) {
      throw new Error(`Invalid target: ${key}`);
    }
    keys.add(key);

    const selfHostedLabels = [...(target.selfHostedRunner ?? expectedSelfHosted)];
    const eligibleForSelfHosted =
      runnerMode === 'self-hosted-all' ||
      (runnerMode === 'auto' && preferredTargets.has(key));
    const useSelfHosted =
      eligibleForSelfHosted &&
      hasOnlineSelfHostedRunner(onlineRunners, selfHostedLabels, faultyRunnerNames);

    resolvedTargets.push({
      ...target,
      runner: useSelfHosted ? selfHostedLabels : expectedHosted,
    });
  }
  if (keys.size !== 6) throw new Error('Expected six OS/CPU targets');
  return {
    ...manifest,
    targets: resolvedTargets,
  };
}

/**
 * Returns the matrix include objects for GitHub Actions (without extra array fields that would
 * clutter matrix job display names).
 */
export function buildMatrixOutputs(effectiveManifest) {
  const include = effectiveManifest.targets.map(
    ({ os, cpu, runner, rustTarget, availability }) => ({
      os,
      cpu,
      runner,
      rustTarget,
      availability,
    }),
  );
  return {
    matrix: { include },
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const manifest = JSON.parse(readFileSync('release-packaging/quality/targets.json', 'utf8'));
  const onlineRunners = queryOnlineRunners();
  const faultyRunners = queryFaultyRunners();
  const effective = buildManifest(manifest, { onlineRunners, faultyRunners });
  const { matrix } = buildMatrixOutputs(effective);
  mkdirSync('quality-output', { recursive: true });
  writeFileSync('quality-output/targets.json', JSON.stringify(effective, null, 2) + '\n');
  if (process.env.GITHUB_OUTPUT) {
    appendFileSync(process.env.GITHUB_OUTPUT, `matrix=${JSON.stringify(matrix)}\n`);
  }
  if (process.env.GITHUB_STEP_SUMMARY) {
    const lines = [
      '### Target runner selection (real-time probe)',
      '',
      '| Target | Rust triple | Selected runner | Mode |',
      '| --- | --- | --- | --- |',
      ...matrix.include.map((t) => {
        const isSelfHosted = Array.isArray(t.runner);
        const label = isSelfHosted ? t.runner.join(', ') : t.runner;
        const mode = isSelfHosted ? 'self-hosted (online)' : 'github-hosted (fallback)';
        return `| ${t.os}-${t.cpu} | \`${t.rustTarget}\` | \`${label}\` | ${mode} |`;
      }),
    ];
    if (faultyRunners.length > 0) {
      lines.push(
        '',
        `Fallback triggered for faulty self-hosted runner(s) that failed \`Set up job\`: \`${faultyRunners.join(', ')}\`.`,
      );
    }
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, lines.join('\n') + '\n');
  }
  console.log(JSON.stringify(matrix));
}
