import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { allowedLicense } from './license-policy.mjs';
const shipped = new Set(['MIT','MIT-0','Apache-2.0','Apache-2.0 WITH LLVM-exception','BSD-2-Clause','BSD-3-Clause','ISC','Zlib','0BSD','Unicode-3.0','Unicode-DFS-2016','BSL-1.0','CC0-1.0']);
const build = new Set([...shipped, 'BlueOak-1.0.0', 'MPL-2.0']);
const root = JSON.parse(readFileSync('gui-web/package.json', 'utf8'));
if (root.name !== 'diskunfold-gui' || root.license !== 'MIT') throw new Error('Project root must declare its expected name and MIT license');
const lock = JSON.parse(readFileSync('gui-web/package-lock.json', 'utf8'));
for (const [path, pkg] of Object.entries(lock.packages)) if (path && (pkg.name === root.name || path.endsWith('/node_modules/' + root.name) || path === 'node_modules/' + root.name) && pkg.version === root.version) throw new Error('External dependency collides with project identity');
const bin = 'gui-web/node_modules/license-checker-rseidelsohn/bin/license-checker-rseidelsohn.js';
mkdirSync('quality-output', {recursive:true});
for (const [name, allow, options] of [['production', shipped, ['--production']], ['all', build, []]]) {
  const result = spawnSync(process.execPath, [bin, '--start', 'gui-web', '--json', ...options], {encoding:'utf8', maxBuffer:100*1024*1024});
  if (result.error || result.status !== 0) { console.error(result.stderr || result.error); process.exitCode = 1; continue; }
  const inventory = JSON.parse(result.stdout);
  const rootKey = `${root.name}@${root.version}`;
  const project = inventory[rootKey];
  if (!project || resolve(project.path) !== resolve('gui-web')) throw new Error('Checker did not identify the exact project root');
  delete inventory[rootKey];
  const violations = Object.entries(inventory).filter(([, p]) => !allowedLicense(p.licenses, allow)).map(([packageName,p]) => ({package:packageName, license:p.licenses, path:p.path}));
  writeFileSync(`quality-output/npm-licenses-${name}.json`, JSON.stringify({inventory, violations}, null, 2) + '\n');
  if (violations.length) { console.error(`${name} license violations: ${JSON.stringify(violations)}`); process.exitCode = 1; }
}
