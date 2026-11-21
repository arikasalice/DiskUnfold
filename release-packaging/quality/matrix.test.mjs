import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import {
  HOSTED_RUNNERS,
  SELF_HOSTED_RUNNERS,
  buildManifest,
  buildMatrixOutputs,
  queryFaultyRunners,
  queryOnlineRunners,
} from './matrix.mjs';

const manifest = JSON.parse(readFileSync(new URL('./targets.json', import.meta.url), 'utf8'));

test('the six-target manifest defaults to GitHub-hosted runners when no self-hosted runner is online', () => {
  assert.deepEqual(
    Object.fromEntries(manifest.targets.map((target) => [`${target.os}-${target.cpu}`, target.runner])),
    HOSTED_RUNNERS,
  );
  assert.deepEqual(
    Object.fromEntries(
      manifest.targets.map((target) => [`${target.os}-${target.cpu}`, target.selfHostedRunner]),
    ),
    SELF_HOSTED_RUNNERS,
  );
  const resolved = buildManifest(manifest, { runnerMode: 'auto' });
  assert.deepEqual(resolved, manifest);
  const outputs = buildMatrixOutputs(resolved);
  assert.equal(outputs.matrix.include.length, 6);
  for (const entry of outputs.matrix.include) {
    assert.equal(entry.selfHostedRunner, undefined);
  }
});

test('online self-hosted runners are selected for slow targets in auto mode while fast or offline targets use GitHub-hosted', () => {
  const onlineRunners = [
    {
      name: 'win-intel-x64',
      status: 'online',
      labels: [{ name: 'self-hosted' }, { name: 'Windows' }, { name: 'X64' }],
    },
    {
      name: 'mac-m1-intel',
      status: 'online',
      labels: [{ name: 'self-hosted' }, { name: 'macOS' }, { name: 'X64' }],
    },
    {
      name: 'mac-m1-linux-x64',
      status: 'online',
      labels: [{ name: 'self-hosted' }, { name: 'Linux' }, { name: 'X64' }],
    },
    {
      name: 'mac-m1-arm',
      status: 'online',
      labels: [{ name: 'self-hosted' }, { name: 'macOS' }, { name: 'ARM64' }],
    },
  ];
  const resolved = buildManifest(manifest, {
    runnerMode: 'auto',
    onlineRunners,
  });
  const byKey = Object.fromEntries(
    resolved.targets.map((target) => [`${target.os}-${target.cpu}`, target.runner]),
  );
  assert.deepEqual(byKey['windows-x64'], ['self-hosted', 'Windows', 'X64']);
  assert.deepEqual(byKey['macos-x64'], ['self-hosted', 'macOS', 'X64']);
  // Fast GitHub-hosted targets stay on dedicated GitHub VMs in auto mode so they do not overload one local host.
  assert.equal(byKey['macos-arm64'], 'macos-15');
  assert.equal(byKey['linux-x64'], 'ubuntu-24.04');
  assert.equal(byKey['linux-arm64'], 'ubuntu-24.04-arm');
  assert.equal(byKey['windows-arm64'], 'windows-11-arm');

  const allSelfHosted = buildManifest(manifest, {
    runnerMode: 'self-hosted-all',
    onlineRunners,
    faultyRunners: ['mac-m1-linux-x64'],
  });
  const byKeyAll = Object.fromEntries(
    allSelfHosted.targets.map((target) => [`${target.os}-${target.cpu}`, target.runner]),
  );
  assert.deepEqual(byKeyAll['macos-arm64'], ['self-hosted', 'macOS', 'ARM64']);
  assert.equal(byKeyAll['linux-x64'], 'ubuntu-24.04');

  const forcedHosted = buildManifest(manifest, {
    runnerMode: 'github-hosted',
    onlineRunners,
  });
  for (const target of forcedHosted.targets) {
    assert.equal(target.runner, HOSTED_RUNNERS[`${target.os}-${target.cpu}`]);
  }
});

test('queryOnlineRunners and queryFaultyRunners handle API responses and errors cleanly', () => {
  assert.equal(queryOnlineRunners({ repository: '', token: '' }), null);
  assert.deepEqual(queryFaultyRunners({ repository: '' }), []);
  assert.equal(
    queryOnlineRunners({
      repository: 'owner/repo',
      token: 'tok',
      run: () => {
        throw new Error('403 Forbidden');
      },
    }),
    null,
  );
  const runners = queryOnlineRunners({
    repository: 'owner/repo',
    token: 'tok',
    run: () => JSON.stringify({ runners: [{ id: 1, status: 'online', labels: [] }] }),
  });
  assert.equal(runners?.length, 1);

  const faulty = queryFaultyRunners({
    repository: 'owner/repo',
    token: 'tok',
    currentRunId: '100',
    run: (_cmd, args) => {
      if (args[1].includes('/workflows/quality.yml/runs')) {
        return JSON.stringify({ workflow_runs: [{ id: 99, status: 'completed' }] });
      }
      return JSON.stringify({
        jobs: [
          {
            runner_name: 'broken-runner',
            status: 'completed',
            steps: [{ name: 'Set up job', conclusion: 'failure' }],
          },
          {
            runner_name: 'healthy-runner',
            status: 'completed',
            steps: [{ name: 'Set up job', conclusion: 'success' }],
          },
        ],
      });
    },
  });
  assert.deepEqual(faulty, ['broken-runner']);
});

test('unknown runner labels and malformed self-hosted definitions are rejected', () => {
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
  const badSelfHosted = {
    ...manifest,
    targets: manifest.targets.map((target) => ({
      ...target,
      selfHostedRunner:
        target.os === 'linux' && target.cpu === 'x64' ? ['self-hosted', 'custom'] : target.selfHostedRunner,
    })),
  };
  assert.throws(() => buildManifest(badSelfHosted), /Invalid target: linux-x64/);
  assert.throws(() => buildManifest(manifest, { runnerMode: 'invalid' }), /Invalid runner mode/);
});
