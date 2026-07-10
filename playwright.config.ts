import { defineConfig, devices } from '@playwright/test'

// FEM v0.2.22 E2E QA — browser-mode via `npm run dev`.
// isTauri() guard in src/lib/tauri.ts causes hooks to fall back to mocks,
// so all UI/UX/routing/state paths exercise without a real Tauri backend.
// Real Tauri IPC verification lives in Phase 4 backend checks (hardwareapi + MongoDB).
export default defineConfig({
  testDir: './tests/e2e',
  timeout: 60_000,
  expect: { timeout: 8_000 },
  fullyParallel: false, // wizard + integrations state is stateful
  workers: 1,
  reporter: [['list'], ['html', { outputFolder: 'playwright-report', open: 'never' }]],
  use: {
    baseURL: 'http://localhost:5173',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
    actionTimeout: 8_000,
    navigationTimeout: 45_000,
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
  webServer: {
    command: 'npm run dev',
    port: 5173,
    reuseExistingServer: false,
    timeout: 60_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
})
// REVERSAL: rm playwright.config.ts
