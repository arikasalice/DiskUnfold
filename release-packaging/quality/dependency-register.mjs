import { readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
const cargo = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--all-features', '--format-version', '1'], {encoding:'utf8', maxBuffer: 100 * 1024 * 1024}));
const ids = new Set(cargo.workspace_members), byId = new Map(cargo.packages.map(p => [p.id, p]));
const entries = [];
for (const node of cargo.resolve.nodes.filter(n => ids.has(n.id))) {
  const owner = byId.get(node.id);
  for (const dep of node.deps) {
    const p = byId.get(dep.pkg);
    if (p.source) entries.push({ecosystem:'cargo', owner:owner.name, name:p.name, version:p.version, source:p.source, license:p.license});
  }
}
const npm = JSON.parse(readFileSync('gui-web/package-lock.json', 'utf8'));
const root = npm.packages[''];
for (const name of Object.keys({...root.dependencies, ...root.devDependencies, ...root.optionalDependencies})) {
  const p = npm.packages[`node_modules/${name}`];
  if (!p?.version || !p.resolved?.startsWith('https://registry.npmjs.org/')) throw new Error(`Missing or non-registry npm dependency: ${name}`);
  entries.push({ecosystem:'npm', owner:root.name, name, version:p.version, source:p.resolved, license:p.license});
}
const metadata = JSON.parse(readFileSync('release-packaging/quality/dependency-annotations.json', 'utf8'));
const keys = new Set();
for (const entry of entries) {
  const key = `${entry.ecosystem}:${entry.owner}:${entry.name}@${entry.version}`;
  const note = metadata[key]; keys.add(key);
  if (!entry.license || !note || !['runtime','build','test'].includes(note.role) || typeof note.distributed !== 'boolean' || !note.reason?.trim() || note.license !== entry.license) throw new Error(`Missing, invalid, or stale dependency annotation: ${key}`);
  Object.assign(entry, {role:note.role, distributed:note.distributed, reason:note.reason});
}
for (const key of Object.keys(metadata)) if (!keys.has(key)) throw new Error(`Stale dependency annotation: ${key}`);
entries.sort((a,b) => JSON.stringify(a).localeCompare(JSON.stringify(b), 'en'));
const data = JSON.stringify(entries, null, 2) + '\n';
const path = 'release-packaging/quality/direct-dependencies.json';
if (process.argv.includes('--write')) writeFileSync(path, data);
else if (readFileSync(path, 'utf8') !== data) { console.error(`Dependency register differs: regenerate ${path}`); process.exitCode = 1; }
