import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { buildManifest, selectTargets } from './matrix.mjs';

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

test('PR and full tiers select the same six targets', () => {
  assert.deepEqual(selectTargets(manifest, 'pr'), selectTargets(manifest, 'full'));
  assert.equal(selectTargets(manifest, 'pr').length, 6);
});

test('unknown tiers are rejected', () => {
  assert.throws(() => selectTargets(manifest, 'nightly'), new Error('Unknown tier: nightly'));
});

test('a manifest without PR targets for every OS is rejected', () => {
  const missingMacosTargets = {
    ...manifest,
    targets: manifest.targets.map((target) => {
      if (target.os !== 'macos') return target;
      const withoutPrCheck = { ...target };
      delete withoutPrCheck.prCheck;
      return withoutPrCheck;
    }),
  };
  assert.throws(
    () => buildManifest(missingMacosTargets),
    new Error('Expected a PR target for each OS: linux, windows, macos'),
  );
});

test('non-boolean prCheck values are rejected', () => {
  const nonBoolean = {
    ...manifest,
    targets: manifest.targets.map((target) => ({
      ...target,
      prCheck: target.os === 'linux' && target.cpu === 'x64' ? 'true' : target.prCheck,
    })),
  };
  assert.throws(() => buildManifest(nonBoolean), /Invalid prCheck: linux-x64/);
});
