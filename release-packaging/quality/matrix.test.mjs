import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { buildManifest } from './matrix.mjs';

const manifest = JSON.parse(readFileSync(new URL('./targets.json', import.meta.url), 'utf8'));
const expectedRunners = {
  'linux-x64': 'ubuntu-24.04',
  'linux-arm64': 'ubuntu-24.04-arm',
  'windows-x64': 'windows-2022',
  'windows-arm64': 'windows-11-arm',
  'macos-x64': 'macos-15-intel',
  'macos-arm64': 'macos-15',
};

test('the six-target manifest keeps its GitHub-hosted runner matrix', () => {
  assert.deepEqual(
    Object.fromEntries(manifest.targets.map((target) => [`${target.os}-${target.cpu}`, target.runner])),
    expectedRunners,
  );
  assert.deepEqual(buildManifest(manifest), manifest);
});

test('self-hosted and unknown runner labels are rejected', () => {
  for (const runner of ['self-hosted', ['self-hosted', 'macOS', 'ARM64'], 'custom']) {
    const candidate = {
      ...manifest,
      targets: manifest.targets.map((target) => ({
        ...target,
        runner: target.os === 'macos' && target.cpu === 'arm64' ? runner : target.runner,
      })),
    };
    assert.throws(() => buildManifest(candidate), /Invalid target: macos-arm64/);
  }
});
