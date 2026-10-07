import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('.', import.meta.url)), '..');
const frontend = join(root, 'frontend');
const backend = join(root, 'backend');
const marker = join(root, 'e2e', '.run-data');

export function prepare() {
  if (existsSync(marker)) return;
  if (!existsSync(join(frontend, 'dist'))) execFileSync('npm', ['--prefix', frontend, 'run', 'build'], { stdio: 'inherit' });
  const dataDir = mkdtempSync(join(tmpdir(), 'pitcairn-e2e-'));
  const env = { ...process.env, PITCAIRN_DATA_DIR: dataDir };
  if (existsSync('/Users/oleghasjanov/projects/pitcairn/backend/target')) env.CARGO_TARGET_DIR = '/Users/oleghasjanov/projects/pitcairn/backend/target';
  execFileSync('cargo', ['run', '--manifest-path', join(backend, 'Cargo.toml'), '--', 'seed-demo', '--reset'], { env, stdio: 'inherit' });
  writeFileSync(marker, dataDir);
}
