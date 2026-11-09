import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync, rmSync } from 'node:fs';
mkdirSync('quality-output', {recursive:true});
rmSync('quality-output/rust-lcov.info', {force:true});
rmSync('quality-output/rust-tests.log', {force:true});
const run = spawnSync('cargo', ['llvm-cov', '--locked', '--workspace', '--all-features', '--lcov', '--output-path', 'quality-output/rust-lcov.info'], {encoding:'utf8', maxBuffer:100 * 1024 * 1024, shell:process.platform === 'win32'});
process.stdout.write(run.stdout || ''); process.stderr.write(run.stderr || '');
writeFileSync('quality-output/rust-tests.log', (run.stdout || '') + '\n' + (run.stderr || ''));
if (run.error || run.status !== 0) { console.error(run.error || `Rust coverage failed: ${run.status}`); process.exitCode = 1; }
