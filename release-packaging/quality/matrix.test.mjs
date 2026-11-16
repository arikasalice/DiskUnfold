import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { buildManifest } from './matrix.mjs';

const manifest = JSON.parse(readFileSync(new URL('./targets.json', import.meta.url), 'utf8'));
const runnerOf = (result, key) => result.targets.find((t) => `${t.os}-${t.cpu}` === key).runner;
const labels = ['self-hosted', 'macOS', 'ARM64', 'du-m1-vm'];

test('valid override replaces only the named runners', () => {
  const result = buildManifest(manifest, JSON.stringify({ 'macos-arm64': labels, 'linux-x64': 'custom' }));
  assert.deepEqual(runnerOf(result, 'macos-arm64'), labels);
  assert.equal(runnerOf(result, 'linux-x64'), 'custom');
  assert.equal(runnerOf(result, 'macos-x64'), runnerOf(manifest, 'macos-x64'));
  assert.equal(runnerOf(manifest, 'macos-arm64'), 'macos-15');
});

test('unset, empty and blank variables keep the hosted runners', () => {
  for (const value of [undefined, '', '  \n']) assert.deepEqual(buildManifest(manifest, value), manifest);
});

test('unknown target key is rejected', () => {
  assert.throws(() => buildManifest(manifest, '{"macos-arm63":"x"}'), /unknown target "macos-arm63"/);
});

test('bad values are rejected', () => {
  for (const bad of ['{"macos-arm64":""}', '{"macos-arm64":[]}', '{"macos-arm64":["a",""]}', '{"macos-arm64":["a",1]}', '{"macos-arm64":5}', '{"macos-arm64":null}', '[]', '"x"', 'null', '{bad']) {
    assert.throws(() => buildManifest(manifest, bad), /DU_RUNNER_OVERRIDES/, bad);
  }
});
