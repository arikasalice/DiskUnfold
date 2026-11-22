import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const list = JSON.parse(readFileSync(new URL('./windows-ui-smoke.json', import.meta.url), 'utf8'));
const smoke = readFileSync(new URL('../launcher/smoke-windows.ps1', import.meta.url), 'utf8');
const orchestrator = readFileSync(new URL('./windows-ui-smoke.ps1', import.meta.url), 'utf8');
const declared = new Set([...smoke.matchAll(/\[switch\]\$(\w+)/g)].map((match) => match[1]));
const folder = (s) =>
  `${s.name}-${s.locale ?? 'en'}-${s.theme ?? 'light'}-${s.windowSize ?? 'default'}` +
  (s.appScale !== undefined && s.appScale !== 100 ? `-scale${s.appScale}` : '');

test('scenario names are unique file-safe words and so are their capture folders', () => {
  const names = list.scenarios.map((scenario) => scenario.name);
  assert.ok(names.length > 0);
  assert.equal(new Set(names).size, names.length);
  for (const name of names) assert.match(name, /^[a-z0-9]+(-[a-z0-9]+)*$/);
  const folders = list.scenarios.map(folder);
  assert.equal(new Set(folders).size, folders.length);
});

test('scenario arguments are switches that smoke-windows.ps1 declares', () => {
  for (const scenario of list.scenarios) {
    for (const argument of scenario.arguments) {
      assert.match(argument, /^-\w+$/, scenario.name);
      assert.ok(declared.has(argument.slice(1)), `${scenario.name}: ${argument} is not a switch of smoke-windows.ps1`);
    }
  }
});

test('scenario language, theme and window size are values smoke-windows.ps1 accepts', () => {
  for (const scenario of list.scenarios) {
    if (scenario.locale !== undefined) assert.ok(['en', 'ko'].includes(scenario.locale), scenario.name);
    if (scenario.theme !== undefined) assert.ok(['light', 'dark', 'contrast'].includes(scenario.theme), scenario.name);
    if (scenario.windowSize !== undefined) assert.match(scenario.windowSize, /^\d+x\d+$/, scenario.name);
    if (scenario.appScale !== undefined) assert.ok([100, 125, 150, 200].includes(scenario.appScale), scenario.name);
  }
  assert.match(smoke, /ValidateSet\('en', 'ko'\)\]\[string\]\$Locale/);
  assert.match(smoke, /ValidateSet\('light', 'dark', 'contrast'\)\]\[string\]\$Theme/);
  assert.match(smoke, /ValidateSet\('100', '125', '150', '200'\)\]\[string\]\$AppScale/);
  assert.match(smoke, /\[string\]\$WindowSize/);
});

test('smoke-windows.ps1 takes the capture folder and the keep-fixture switch', () => {
  assert.match(smoke, /\[string\]\$CaptureDirectory/);
  assert.ok(declared.has('KeepFixture'));
});

test('UI smoke shards declare bounded parameters and distribute filtered scenario indexes', () => {
  assert.match(orchestrator, /\[ValidateRange\(1,\s*20\)\]\[int\]\$ShardCount\s*=\s*1/);
  assert.match(orchestrator, /\[ValidateRange\(1,\s*20\)\]\[int\]\$Shard\s*=\s*1/);
  assert.match(orchestrator, /\$index\s*%\s*\$ShardCount\)\s*-eq\s*\(\$Shard\s*-\s*1\)/);
});

test('three UI smoke shards assign every scenario once with balanced sizes', () => {
  const shardCount = 3;
  const shards = Array.from({ length: shardCount }, (_, shardIndex) =>
    list.scenarios.filter((_, index) => index % shardCount === shardIndex),
  );
  for (const scenario of list.scenarios) {
    assert.equal(shards.filter((shard) => shard.includes(scenario)).length, 1, scenario.name);
  }
  const sizes = shards.map((shard) => shard.length);
  assert.ok(Math.max(...sizes) - Math.min(...sizes) <= 1);
});
