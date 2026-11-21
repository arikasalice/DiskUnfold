import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { buildManifest, resolveRunners, selectTargets } from './matrix.mjs';

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

const targets = selectTargets(manifest, 'full');
const apiRunnerResponse = ({ armStatus = 'online', intelStatus = 'online', armBusy = false, intelBusy = false } = {}) => ({
  runners: [
    {
      name: 'mac-m1-arm',
      os: 'macOS',
      status: armStatus,
      busy: armBusy,
      labels: [
        { id: 1, name: 'self-hosted', type: 'read-only' },
        { name: 'macOS' },
        { name: 'ARM64' },
      ],
    },
    {
      name: 'mac-m1-intel',
      os: 'macOS',
      status: intelStatus,
      busy: intelBusy,
      labels: [
        { id: 2, name: 'self-hosted', type: 'read-only' },
        { name: 'macOS' },
        { name: 'X64' },
      ],
    },
  ],
});

function kinds(runners, options) {
  return Object.fromEntries(resolveRunners(targets, runners, options)
    .map((target) => [`${target.os}-${target.cpu}`, target.runnerKind]));
}

test('auto selects the online idle ARM runner when Intel is offline', () => {
  assert.deepEqual(kinds(apiRunnerResponse({ intelStatus: 'offline' }).runners), {
    'linux-x64': 'github-hosted',
    'linux-arm64': 'github-hosted',
    'windows-x64': 'github-hosted',
    'windows-arm64': 'github-hosted',
    'macos-x64': 'github-hosted',
    'macos-arm64': 'self-hosted',
  });
});

test('auto selects only the lower-priority-number target when both runners are online', () => {
  const result = kinds(apiRunnerResponse().runners);
  assert.equal(result['macos-x64'], 'self-hosted');
  assert.equal(result['macos-arm64'], 'github-hosted');
});

test('a busy runner makes its whole group use GitHub-hosted runners', () => {
  const result = kinds(apiRunnerResponse({ armBusy: true }).runners);
  assert.equal(result['macos-x64'], 'github-hosted');
  assert.equal(result['macos-arm64'], 'github-hosted');
});

test('offline, absent and malformed runner lists all fall back to hosted runners', () => {
  for (const runners of [
    apiRunnerResponse({ armStatus: 'offline', intelStatus: 'offline' }).runners,
    [],
    { runners: [] },
  ]) {
    const result = kinds(runners);
    assert.equal(result['macos-x64'], 'github-hosted');
    assert.equal(result['macos-arm64'], 'github-hosted');
  }
});

test('untrusted events and github-hosted mode always use hosted runners', () => {
  for (const result of [
    kinds(apiRunnerResponse().runners, { trusted: false }),
    kinds(apiRunnerResponse().runners, { mode: 'github-hosted' }),
  ]) {
    assert.ok(Object.values(result).every((kind) => kind === 'github-hosted'));
  }
});

test('force mode selects one configured target without runner data and validates keys and groups', () => {
  const result = resolveRunners(targets, [], { mode: 'force:macos-arm64' });
  assert.equal(result.find((target) => target.os === 'macos' && target.cpu === 'arm64').runnerKind, 'self-hosted');
  const withoutSelfHostedConfig = targets.map((target) => {
    const { selfHosted, ...rest } = target;
    return target.os === 'macos' && target.cpu === 'arm64' ? rest : target;
  });
  assert.equal(
    resolveRunners(withoutSelfHostedConfig, [], { mode: 'force:macos-arm64' })
      .find((target) => target.os === 'macos' && target.cpu === 'arm64').runnerKind,
    'github-hosted',
  );
  assert.throws(
    () => resolveRunners(targets, [], { mode: 'force:macos-x64,macos-arm64' }),
    new Error('force allows one target per group'),
  );
  assert.throws(
    () => resolveRunners(targets, [], { mode: 'force:macos-ppc64' }),
    new Error('Unknown force target: macos-ppc64'),
  );
  assert.throws(
    () => resolveRunners(targets, [], { mode: 'sometimes' }),
    new Error('Unknown runner mode: sometimes'),
  );
});

test('runner labels match case-insensitively and accept string labels, while incomplete labels are ignored', () => {
  const runners = [
    {
      name: 'arm-string-labels',
      status: 'online',
      busy: false,
      labels: ['SELF-HOSTED', { name: 'macos' }, 'arm64'],
    },
    {
      name: 'incomplete-intel',
      status: 'online',
      busy: false,
      labels: ['self-hosted', 'macos'],
    },
  ];
  const result = kinds(runners);
  assert.equal(result['macos-arm64'], 'self-hosted');
  assert.equal(result['macos-x64'], 'github-hosted');
});

test('resolved entries omit manifest-only fields and keep Linux and Windows hosted', () => {
  const result = resolveRunners(targets, apiRunnerResponse().runners);
  assert.deepEqual(
    result.map((target) => `${target.os}-${target.cpu}`),
    targets.map((target) => `${target.os}-${target.cpu}`),
  );
  for (const target of result) {
    assert.equal('prCheck' in target, false);
    assert.equal('selfHosted' in target, false);
    assert.equal(typeof target.runsOn, 'string');
    assert.ok(Array.isArray(JSON.parse(target.runsOn)));
    if (target.os === 'linux' || target.os === 'windows') {
      assert.equal(target.runnerKind, 'github-hosted');
      assert.equal(target.runsOn, JSON.stringify([target.runner]));
    }
  }
});

test('selfHosted validation rejects non-macOS targets and malformed labels, groups or priorities', () => {
  const withConfig = (key, config) => ({
    ...manifest,
    targets: manifest.targets.map((target) => (
      `${target.os}-${target.cpu}` === key ? { ...target, selfHosted: config } : target
    )),
  });
  const config = { labels: ['self-hosted', 'macOS', 'X64'], group: 'mac-m1', priority: 1 };
  assert.throws(() => buildManifest(withConfig('linux-x64', config)), new Error('Invalid selfHosted: linux-x64'));
  assert.throws(() => buildManifest(withConfig('macos-x64', { ...config, labels: ['self-hosted'] })), new Error('Invalid selfHosted: macos-x64'));
  assert.throws(() => buildManifest(withConfig('macos-x64', { ...config, group: '  ' })), new Error('Invalid selfHosted: macos-x64'));
  assert.throws(() => buildManifest(withConfig('macos-x64', { ...config, priority: 1.5 })), new Error('Invalid selfHosted: macos-x64'));
});
