import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const list = JSON.parse(readFileSync(new URL('./windows-ui-smoke.json', import.meta.url), 'utf8'));
const smoke = readFileSync(new URL('../launcher/smoke-windows.ps1', import.meta.url), 'utf8');
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

test('scenario language, theme, tier and window size are values smoke-windows.ps1 accepts', () => {
  for (const scenario of list.scenarios) {
    assert.ok(['smoke', 'gallery'].includes(scenario.tier), `${scenario.name}: invalid tier`);
    if (scenario.locale !== undefined) assert.ok(['en', 'ko'].includes(scenario.locale), scenario.name);
    if (scenario.theme !== undefined) assert.ok(['light', 'dark', 'contrast'].includes(scenario.theme), scenario.name);
    if (scenario.windowSize !== undefined) assert.match(scenario.windowSize, /^\d+x\d+$/, scenario.name);
    if (scenario.appScale !== undefined) assert.ok([100, 125, 150, 200].includes(scenario.appScale), scenario.name);
  }
  const smokeNames = new Set(list.scenarios.filter((s) => s.tier === 'smoke').map((s) => s.name));
  for (const required of ['functional', 'virtual-groups', 'virtual-groups-sample']) {
    assert.ok(smokeNames.has(required), `smoke tier must include ${required}`);
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
