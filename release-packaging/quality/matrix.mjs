import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const HOSTED_RUNNERS = Object.freeze({
  'linux-x64': 'ubuntu-24.04',
  'linux-arm64': 'ubuntu-24.04-arm',
  'windows-x64': 'windows-2022',
  'windows-arm64': 'windows-11-arm',
  'macos-x64': 'macos-15-intel',
  'macos-arm64': 'macos-15',
});

const SELF_HOSTED_LABELS = Object.freeze({
  'macos-x64': ['self-hosted', 'macOS', 'X64'],
  'macos-arm64': ['self-hosted', 'macOS', 'ARM64'],
});

const USAGE = 'Usage: node release-packaging/quality/matrix.mjs [--tier pr|full] [--runners <file>] [--mode <mode>] [--untrusted]';

function targetKey(target) {
  return `${target.os}-${target.cpu}`;
}

/**
 * Validates the six-target manifest and restricts hosted and self-hosted runner labels.
 * Throws on unknown targets, invalid runner configuration or malformed values.
 */
export function buildManifest(manifest) {
  const keys = new Set();
  const prTargetOs = new Set();
  for (const target of manifest.targets) {
    const key = targetKey(target);
    if ('selfHosted' in target) {
      const allowedLabels = Object.hasOwn(SELF_HOSTED_LABELS, key) ? SELF_HOSTED_LABELS[key] : undefined;
      const config = target.selfHosted;
      if (
        !allowedLabels
        || !config
        || !Array.isArray(config.labels)
        || config.labels.length !== allowedLabels.length
        || config.labels.some((label, index) => label !== allowedLabels[index])
        || typeof config.group !== 'string'
        || config.group.trim().length === 0
        || !Number.isInteger(config.priority)
      ) {
        throw new Error(`Invalid selfHosted: ${key}`);
      }
    }
    if (keys.has(key) || target.runner !== HOSTED_RUNNERS[key] || !target.rustTarget) {
      throw new Error(`Invalid target: ${key}`);
    }
    if ('prCheck' in target && typeof target.prCheck !== 'boolean') {
      throw new Error(`Invalid prCheck: ${key}`);
    }
    if (target.prCheck === true) prTargetOs.add(target.os);
    keys.add(key);
  }
  if (keys.size !== 6) throw new Error('Expected six OS/CPU targets');
  if (['linux', 'windows', 'macos'].some((os) => !prTargetOs.has(os))) {
    throw new Error('Expected a PR target for each OS: linux, windows, macos');
  }
  return manifest;
}

/** Selects manifest targets for a quality tier. Throws for unknown tiers. */
export function selectTargets(manifest, tier) {
  if (tier === 'pr') return manifest.targets.filter((target) => target.prCheck === true);
  if (tier === 'full') return manifest.targets;
  throw new Error(`Unknown tier: ${tier}`);
}

function runnerLabels(runner) {
  if (!Array.isArray(runner?.labels)) return [];
  return runner.labels
    .map((label) => (typeof label === 'string' ? label : label?.name))
    .filter((label) => typeof label === 'string')
    .map((label) => label.toLowerCase());
}

function hasLabels(runner, requiredLabels) {
  const available = new Set(runnerLabels(runner));
  return requiredLabels.every((label) => available.has(label.toLowerCase()));
}

function resolveDecisions(targets, runners, mode, trusted) {
  if (mode !== 'auto' && mode !== 'github-hosted' && !(typeof mode === 'string' && mode.startsWith('force:'))) {
    throw new Error(`Unknown runner mode: ${mode}`);
  }
  const isForceMode = mode.startsWith('force:');

  const decisions = new Map();
  const selectedKeys = new Set();
  if (isForceMode) {
    const forceKeys = new Set(mode.slice('force:'.length).split(','));
    for (const key of forceKeys) {
      if (!Object.hasOwn(SELF_HOSTED_LABELS, key)) throw new Error(`Unknown force target: ${key}`);
      selectedKeys.add(key);
    }
    const selectedGroups = new Set();
    for (const target of targets) {
      const key = targetKey(target);
      if (!selectedKeys.has(key)) continue;
      const group = target.selfHosted?.group;
      if (group && selectedGroups.has(group)) throw new Error('force allows one target per group');
      if (group) selectedGroups.add(group);
    }
  }

  if (!trusted || mode === 'github-hosted') {
    const reason = trusted ? 'github-hosted mode' : 'untrusted event';
    for (const target of targets) decisions.set(targetKey(target), { kind: 'github-hosted', reason });
    return decisions;
  }

  if (isForceMode) {
    for (const target of targets) {
      const forced = Boolean(target.selfHosted) && selectedKeys.has(targetKey(target));
      decisions.set(targetKey(target), {
        kind: forced ? 'self-hosted' : 'github-hosted',
        reason: forced ? 'forced' : 'not selected by force mode',
      });
    }
    return decisions;
  }

  const groups = new Map();
  for (const target of targets) {
    if (!target.selfHosted) {
      decisions.set(targetKey(target), { kind: 'github-hosted', reason: 'no self-hosted configuration' });
      continue;
    }
    const groupTargets = groups.get(target.selfHosted.group) ?? [];
    groupTargets.push(target);
    groups.set(target.selfHosted.group, groupTargets);
  }

  for (const [group, groupTargets] of groups) {
    const busy = runners.some((runner) => runner?.busy === true
      && groupTargets.some((target) => hasLabels(runner, target.selfHosted.labels)));
    if (busy) {
      for (const target of groupTargets) {
        decisions.set(targetKey(target), { kind: 'github-hosted', reason: 'group busy' });
      }
      continue;
    }

    const candidates = groupTargets.flatMap((target, index) => {
      const runner = runners.find((entry) => entry?.status === 'online' && hasLabels(entry, target.selfHosted.labels));
      return runner ? [{ target, runner, index }] : [];
    });
    candidates.sort((left, right) => left.target.selfHosted.priority - right.target.selfHosted.priority
      || left.index - right.index);
    const winner = candidates[0];
    for (const target of groupTargets) {
      if (winner?.target === target) {
        const name = typeof winner.runner.name === 'string' && winner.runner.name.trim()
          ? winner.runner.name
          : group;
        decisions.set(targetKey(target), {
          kind: 'self-hosted',
          reason: `${name} online, idle`,
        });
      } else {
        const hasOnlineRunner = candidates.some((candidate) => candidate.target === target);
        decisions.set(targetKey(target), {
          kind: 'github-hosted',
          reason: hasOnlineRunner ? 'another target selected' : 'no online runner',
        });
      }
    }
  }
  return decisions;
}

/** Resolves target runners while preserving target order and stripping manifest-only fields. */
export function resolveRunners(targets, runners, { mode = 'auto', trusted = true } = {}) {
  const availableRunners = Array.isArray(runners) ? runners : [];
  const decisions = resolveDecisions(targets, availableRunners, mode, trusted);
  return targets.map((target) => {
    const { prCheck, selfHosted, ...resolvedTarget } = target;
    const decision = decisions.get(targetKey(target));
    return {
      ...resolvedTarget,
      runnerKind: decision.kind,
      runsOn: JSON.stringify(decision.kind === 'self-hosted' ? selfHosted.labels : [target.runner]),
    };
  });
}

function parseArgs(args) {
  const options = { tier: 'full', mode: 'auto', trusted: true };
  const seen = new Set();
  for (let index = 0; index < args.length; index += 1) {
    const option = args[index];
    if (option === '--untrusted') {
      if (seen.has(option)) throw new Error(USAGE);
      seen.add(option);
      options.trusted = false;
      continue;
    }
    if (option !== '--tier' && option !== '--runners' && option !== '--mode') throw new Error(USAGE);
    if (seen.has(option) || index + 1 >= args.length || args[index + 1].startsWith('--')) {
      throw new Error(USAGE);
    }
    seen.add(option);
    const value = args[++index];
    if (option === '--tier') {
      if (value !== 'pr' && value !== 'full') throw new Error(USAGE);
      options.tier = value;
    } else if (option === '--runners') {
      options.runnersFile = value;
    } else {
      options.mode = value;
    }
  }
  return options;
}

function readRunners(file) {
  if (file === undefined) return [];
  try {
    const response = JSON.parse(readFileSync(file, 'utf8'));
    if (Array.isArray(response?.runners)) return response.runners;
  } catch {
    // Invalid or unavailable runner data safely falls back to GitHub-hosted runners.
  }
  process.stderr.write('Warning: invalid or unavailable runners file; using no self-hosted runners.\n');
  return [];
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const options = parseArgs(process.argv.slice(2));
  const manifest = buildManifest(JSON.parse(readFileSync('release-packaging/quality/targets.json', 'utf8')));
  const targets = selectTargets(manifest, options.tier);
  const runners = readRunners(options.runnersFile);
  const decisions = resolveDecisions(targets, runners, options.mode, options.trusted);
  const include = resolveRunners(targets, runners, options);
  for (const target of targets) {
    const decision = decisions.get(targetKey(target));
    process.stderr.write(`${targetKey(target)}: ${decision.kind} (${decision.reason})\n`);
  }

  const effective = { ...manifest, targets };
  mkdirSync('quality-output', { recursive: true });
  writeFileSync('quality-output/targets.json', JSON.stringify(effective, null, 2) + '\n');
  process.stdout.write(`${JSON.stringify({ include })}\n`);
}
