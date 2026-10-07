import { spawn } from 'node:child_process';
import { existsSync, readFileSync, rmSync, unlinkSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { prepare } from './prepare.mjs';

try { unlinkSync(new URL('.run-data', import.meta.url)); } catch {}
prepare();
const root = resolve(fileURLToPath(new URL('.', import.meta.url)), '..');
const dataDir = readFileSync(join(root, 'e2e', '.run-data'), 'utf8');
const env = { ...process.env, PITCAIRN_DATA_DIR: dataDir, PITCAIRN_DEMO_MODE: 'true', PITCAIRN_BIND: '127.0.0.1:18080', PITCAIRN_BASE_URL: 'http://127.0.0.1:18080', PITCAIRN_STATIC_DIR: join(root, 'frontend', 'dist') };
const localTarget = '/Users/oleghasjanov/projects/pitcairn/backend/target';
if (process.env.CARGO_TARGET_DIR || existsSync(localTarget)) env.CARGO_TARGET_DIR = process.env.CARGO_TARGET_DIR ?? localTarget;
const server = spawn('cargo', ['run', '--manifest-path', join(root, 'backend', 'Cargo.toml'), '--', 'serve'], { env, stdio: 'inherit' });
function stop() { server.kill('SIGTERM'); }
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
server.on('exit', code => {
  try { unlinkSync(join(root, 'e2e', '.run-data')); } catch {}
  rmSync(dataDir, { recursive: true, force: true });
  process.exit(code ?? 0);
});
