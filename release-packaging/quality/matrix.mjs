import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

/**
 * Validates the six-target manifest and applies the optional runner overrides.
 * `overrides` is the raw DU_RUNNER_OVERRIDES value: a JSON object that maps "<os>-<cpu>" to a
 * runner label (string) or a label list (array). An unset or blank value changes nothing.
 * Throws on unknown targets or malformed values. Returns a manifest whose runners are effective.
 */
export function buildManifest(manifest, overrides) {
  const keys = new Set();
  for (const target of manifest.targets) {
    const key = `${target.os}-${target.cpu}`;
    if (keys.has(key) || !target.runner || !target.rustTarget) throw new Error(`Invalid target: ${key}`);
    keys.add(key);
  }
  if (keys.size !== 6) throw new Error('Expected six OS/CPU targets');
  if (typeof overrides !== 'string' || overrides.trim() === '') return manifest;
  let parsed;
  try {
    parsed = JSON.parse(overrides);
  } catch (error) {
    throw new Error(`DU_RUNNER_OVERRIDES is not valid JSON: ${error.message}`);
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('DU_RUNNER_OVERRIDES must be a JSON object keyed by "<os>-<cpu>"');
  }
  const isLabel = (value) => typeof value === 'string' && value.trim() !== '';
  for (const [key, value] of Object.entries(parsed)) {
    if (!keys.has(key)) throw new Error(`DU_RUNNER_OVERRIDES has unknown target "${key}"`);
    const valid = isLabel(value) || (Array.isArray(value) && value.length > 0 && value.every(isLabel));
    if (!valid) {
      throw new Error(`DU_RUNNER_OVERRIDES["${key}"] must be a non-empty string or a non-empty array of non-empty strings`);
    }
  }
  return {
    ...manifest,
    targets: manifest.targets.map((target) => {
      const key = `${target.os}-${target.cpu}`;
      return Object.hasOwn(parsed, key) ? { ...target, runner: parsed[key] } : target;
    }),
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const manifest = JSON.parse(readFileSync('release-packaging/quality/targets.json', 'utf8'));
  const effective = buildManifest(manifest, process.env.DU_RUNNER_OVERRIDES);
  mkdirSync('quality-output', { recursive: true });
  writeFileSync('quality-output/targets.json', JSON.stringify(effective, null, 2) + '\n');
  console.log(JSON.stringify({ include: effective.targets }));
}
