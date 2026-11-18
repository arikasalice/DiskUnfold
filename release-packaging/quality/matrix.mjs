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
  for (const target of manifest.targets) {
    const key = `${target.os}-${target.cpu}`;
    if (keys.has(key) || target.runner !== HOSTED_RUNNERS[key] || !target.rustTarget) {
      throw new Error(`Invalid target: ${key}`);
    }
    keys.add(key);
  }
  if (keys.size !== 6) throw new Error('Expected six OS/CPU targets');
  return manifest;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const manifest = JSON.parse(readFileSync('release-packaging/quality/targets.json', 'utf8'));
  const effective = buildManifest(manifest);
  mkdirSync('quality-output', { recursive: true });
  writeFileSync('quality-output/targets.json', JSON.stringify(effective, null, 2) + '\n');
  console.log(JSON.stringify({ include: effective.targets }));
}
