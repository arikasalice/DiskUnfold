import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const registry = JSON.parse(readFileSync('release-packaging/quality/fuzz-targets.json', 'utf8'));
const { targets } = registry;
if (!Array.isArray(targets)) throw new Error('Invalid fuzz target registry');
const results = [];
function run(command, args) {
  const r = spawnSync(command, args, {stdio:'inherit', shell:process.platform === 'win32'});
  return !r.error && r.status === 0;
}
if (targets.length) {
  if (!/^nightly-\d{4}-\d{2}-\d{2}$/.test(registry.toolchain) || !/^\d+\.\d+\.\d+$/.test(registry.cargoFuzzVersion)) throw new Error('Pin nightly toolchain and cargo-fuzz version in the registry');
  if (!run('rustup', ['toolchain','install',registry.toolchain,'--profile','minimal']) || !run('cargo', ['install','--locked','cargo-fuzz','--version',registry.cargoFuzzVersion])) throw new Error('Fuzz tools could not be installed');
}
for (const target of targets) {
  if (!/^[\w-]+$/.test(target.name) || !Number.isInteger(target.seconds) || target.seconds < 1) throw new Error('Invalid fuzz target');
  results.push({name:target.name, status:run('cargo', [`+${registry.toolchain}`, 'fuzz', 'run', target.name, '--', `-max_total_time=${target.seconds}`]) ? 'passed' : 'failed'});
}
mkdirSync('quality-output', {recursive:true});
writeFileSync('quality-output/fuzz-report.json', JSON.stringify({status: targets.length ? (results.every(r=>r.status==='passed') ? 'passed' : 'failed') : 'not configured', results}, null, 2) + '\n');
if (results.some(r=>r.status==='failed')) process.exitCode = 1;
