import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const list = JSON.parse(readFileSync(new URL('./windows-ui-smoke.json', import.meta.url), 'utf8'));
const smoke = readFileSync(new URL('../launcher/smoke-windows.ps1', import.meta.url), 'utf8');
const declared = new Set([...smoke.matchAll(/\[switch\]\$(\w+)/g)].map((match) => match[1]));

test('scenario names are unique file-safe words', () => {
  const names = list.scenarios.map((scenario) => scenario.name);
  assert.ok(names.length > 0);
  assert.equal(new Set(names).size, names.length);
  for (const name of names) assert.match(name, /^[a-z0-9]+(-[a-z0-9]+)*$/);
});

test('scenario arguments are switches that smoke-windows.ps1 declares', () => {
  for (const scenario of list.scenarios) {
    for (const argument of scenario.arguments) {
      assert.match(argument, /^-\w+$/, scenario.name);
      assert.ok(declared.has(argument.slice(1)), `${scenario.name}: ${argument} is not a switch of smoke-windows.ps1`);
    }
  }
});

test('smoke-windows.ps1 takes the capture folder and the keep-fixture switch', () => {
  assert.match(smoke, /\[string\]\$CaptureDirectory/);
  assert.ok(declared.has('KeepFixture'));
});
