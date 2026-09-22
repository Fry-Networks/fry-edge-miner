import { defineConfig } from '@playwright/test'

/**
 * FEM QA VM matrix — Playwright config for specs that drive the REAL app
 * inside a Windows guest over CDP, as opposed to tests/e2e/**'s
 * playwright.config.ts (browser-preview mode against a local vite dev
 * server). Deliberately separate: no `webServer` (there is nothing to
 * start — vm-launch-fem already launched the app in the guest before this
 * runs), no `baseURL` (navigation is by clicking the app's own sidebar, not
 * page.goto()), and it must not touch or replace the existing
 * playwright.config.ts (out of fence — see runlog/decisions.md D-01 /
 * the ui-item brief's "no changes to playwright.config.ts").
 *
 * See qa/vm/README.md for prerequisites and exact invocation.
 */
export default defineConfig({
  testDir: '.',
  testMatch: /.*\.spec\.ts/,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  // Evidence beyond the terminal report (per-assertion numbers, screenshots)
  // is written by the specs themselves into ~/femqa/evidence/vm/<matrix>/,
  // matching vm-shot/vm-popups/etc.'s convention — see qa/vm/harness.ts.
  reporter: [['list'], ['html', { outputFolder: 'playwright-report', open: 'never' }]],
  use: {
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    actionTimeout: 10_000,
  },
  projects: [{ name: 'vm-cdp' }],
})
