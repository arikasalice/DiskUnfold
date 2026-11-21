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

function hasOnlineSelfHostedRunner(onlineRunners, requiredLabels) {
  if (!Array.isArray(onlineRunners)) return false;
  const required = requiredLabels.map((label) => String(label).toLowerCase());
  return onlineRunners.some((runner) => {
    if (!runner || runner.status !== 'online' || !Array.isArray(runner.labels)) return false;
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
 * Validates the six-target manifest and resolves each target's active runner in real time.
 * Selects the target's self-hosted runner only when the live runner status confirms an online
 * runner with matching labels, and automatically falls back to the GitHub-hosted runner when
 * the self-hosted runner is offline, unregistered, or unreachable.
 */
export function buildManifest(manifest, options = {}) {
  if (!manifest || !Array.isArray(manifest.targets)) {
    throw new Error('Expected manifest.targets array');
  }
  const runnerMode = (options.runnerMode ?? process.env.DU_RUNNER_MODE ?? 'auto')
    .trim()
    .toLowerCase();
  if (!['auto', 'github-hosted'].includes(runnerMode)) {
    throw new Error(`Invalid runner mode: ${runnerMode}`);
  }
  const onlineRunners = options.onlineRunners ?? null;

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
    const useSelfHosted =
      runnerMode !== 'github-hosted' &&
      hasOnlineSelfHostedRunner(onlineRunners, selfHostedLabels);

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
 * clutter matrix job display names) and the resolved Windows x64 runner for ui-smoke.
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
  const windowsX64 = include.find((target) => target.os === 'windows' && target.cpu === 'x64');
  return {
    matrix: { include },
    uiSmokeRunner: windowsX64 ? windowsX64.runner : HOSTED_RUNNERS['windows-x64'],
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const manifest = JSON.parse(readFileSync('release-packaging/quality/targets.json', 'utf8'));
  const onlineRunners = queryOnlineRunners();
  const effective = buildManifest(manifest, { onlineRunners });
  const { matrix, uiSmokeRunner } = buildMatrixOutputs(effective);
  mkdirSync('quality-output', { recursive: true });
  writeFileSync('quality-output/targets.json', JSON.stringify(effective, null, 2) + '\n');
  if (process.env.GITHUB_OUTPUT) {
    appendFileSync(
      process.env.GITHUB_OUTPUT,
      `matrix=${JSON.stringify(matrix)}\nui_smoke_runner=${JSON.stringify(uiSmokeRunner)}\n`,
    );
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
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, lines.join('\n') + '\n');
  }
  console.log(JSON.stringify(matrix));
}
