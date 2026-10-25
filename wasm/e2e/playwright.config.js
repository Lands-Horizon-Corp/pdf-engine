import { defineConfig, devices } from '@playwright/test';
import os from 'node:os';
import { resolve } from 'node:path';

// The native server (from `cargo build`), allowed to be called from the test pages.
const server = resolve(import.meta.dirname, `../../target/debug/pdf-engine${process.platform === 'win32' ? '.exe' : ''}`);

// Chromium covers Chrome, Edge and Brave; WebKit is Safari's engine; Firefox is Gecko.
export default defineConfig({
  testDir: 'tests',
  timeout: 120_000,
  fullyParallel: true,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list']],
  use: { baseURL: 'http://localhost:4180' },
  webServer: [
    {
      command: 'npm run build && npx vite preview --port 4180 --strictPort',
      url: 'http://localhost:4180/plain/',
      reuseExistingServer: !process.env.CI,
      timeout: 240_000,
    },
    {
      command: server,
      cwd: os.tmpdir(),
      env: { API_PORT: '4181', API_BEARER_TOKEN: 't', CORS_ALLOWED_ORIGINS: 'http://localhost:4180' },
      url: 'http://127.0.0.1:4181/health',
      reuseExistingServer: !process.env.CI,
      timeout: 60_000,
    },
  ],
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});
