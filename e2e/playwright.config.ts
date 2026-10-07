import { fileURLToPath } from 'node:url';
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  timeout: 120_000,
  expect: { timeout: 10_000 },
  workers: 1,
  retries: 0,
  reporter: 'list',
  globalSetup: './global-setup.ts',
  use: { ...devices['Desktop Chrome'], baseURL: 'http://127.0.0.1:18080', actionTimeout: 10_000, navigationTimeout: 15_000, trace: 'retain-on-failure' },
  projects: [{ name: 'chromium', use: { browserName: 'chromium' } }],
  webServer: {
    command: 'exec node serve.mjs',
    cwd: fileURLToPath(new URL('.', import.meta.url)),
    url: 'http://127.0.0.1:18080/up',
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
