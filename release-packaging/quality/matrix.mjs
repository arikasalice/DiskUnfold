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

/**
 * Validates the six-target manifest and restricts each target to its GitHub-hosted runner label.
 * Throws on unknown targets, non-hosted labels or malformed values. Returns the unchanged manifest.
 */
export function buildManifest(manifest) {
  const keys = new Set();
  const prTargets = [];
  for (const target of manifest.targets) {
    const key = `${target.os}-${target.cpu}`;
    if (keys.has(key) || target.runner !== HOSTED_RUNNERS[key] || !target.rustTarget) {
      throw new Error(`Invalid target: ${key}`);
    }
    if ('prCheck' in target && typeof target.prCheck !== 'boolean') {
      throw new Error(`Invalid prCheck: ${key}`);
    }
    if (target.prCheck === true) prTargets.push(key);
    keys.add(key);
  }
  if (keys.size !== 6) throw new Error('Expected six OS/CPU targets');
  if (prTargets.length !== 3 || ['linux-x64', 'windows-x64', 'macos-arm64'].some((key) => !prTargets.includes(key))) {
    throw new Error('Expected PR targets: linux-x64, windows-x64, macos-arm64');
  }
  return manifest;
}

/** Selects manifest targets for a quality tier. Throws for unknown tiers. */
export function selectTargets(manifest, tier) {
  if (tier === 'pr') return manifest.targets.filter((target) => target.prCheck === true);
  if (tier === 'full') return manifest.targets;
  throw new Error(`Unknown tier: ${tier}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.length !== 0 && (args.length !== 2 || args[0] !== '--tier')) {
    throw new Error('Usage: node release-packaging/quality/matrix.mjs [--tier pr|full]');
  }
  const tier = args.length === 2 ? args[1] : 'full';
  const manifest = buildManifest(JSON.parse(readFileSync('release-packaging/quality/targets.json', 'utf8')));
  const effective = { ...manifest, targets: selectTargets(manifest, tier) };
  mkdirSync('quality-output', { recursive: true });
  writeFileSync('quality-output/targets.json', JSON.stringify(effective, null, 2) + '\n');
  console.log(JSON.stringify({ include: effective.targets }));
}
