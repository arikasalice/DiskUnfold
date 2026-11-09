import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
const manifest = JSON.parse(readFileSync('release-packaging/quality/targets.json', 'utf8'));
const keys = new Set();
for (const target of manifest.targets) {
  const key = `${target.os}-${target.cpu}`;
  if (keys.has(key) || !target.runner || !target.rustTarget) throw new Error(`Invalid target: ${key}`);
  keys.add(key);
}
if (keys.size !== 6) throw new Error('Expected six OS/CPU targets');
mkdirSync('quality-output', { recursive: true });
writeFileSync('quality-output/targets.json', JSON.stringify(manifest, null, 2) + '\n');
console.log(JSON.stringify({include: manifest.targets}));
