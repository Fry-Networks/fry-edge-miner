import { test as base, expect, type Page } from '@playwright/test'

// Every test starts with browser errors captured. Any unexpected console
// error, page-error, request failure, or HTTP 4xx/5xx aborts the run via
// afterEach — this is the cross-cutting I1/I2 rule from the QA plan.
type ErrLog = string[]

export const test = base.extend<{ errors: ErrLog }>({
  errors: async ({ page }, use) => {
    const errors: ErrLog = []
    // Known Vite-dev-server ignorable requests (HMR, SW, favicon in dev).
    const IGNORE_URL = /\/(favicon\.ico|__vite_ping|__vite_hmr|@vite\/client|@react-refresh)/
    const IGNORE_MSG = /Extension context invalidated|ResizeObserver loop|MutationObserver/i

    page.on('pageerror', (e) => {
      if (!IGNORE_MSG.test(e.message)) errors.push(`PAGEERROR: ${e.message}`)
    })
    page.on('console', (m) => {
      if (m.type() === 'error' && !IGNORE_MSG.test(m.text())) {
        errors.push(`CONSOLE ERROR: ${m.text()}`)
      }
    })
    page.on('requestfailed', (r) => {
      if (!IGNORE_URL.test(r.url())) {
        errors.push(`REQFAIL: ${r.url()} ${r.failure()?.errorText ?? ''}`)
      }
    })
    page.on('response', (r) => {
      const s = r.status()
      if (s >= 400 && !IGNORE_URL.test(r.url())) {
        errors.push(`HTTP ${s}: ${r.url()}`)
      }
    })
    await use(errors)
  },
})

export { expect }

// The 5 nav labels — must stay in sync with src/components/Sidebar.tsx NAV.
export const NAV_LABELS = ['Dashboard', 'Integrations', 'Rewards', 'Settings', 'Updates'] as const
export type NavLabel = (typeof NAV_LABELS)[number]

// 5 integration display names as defined in src/lib/integrationMeta.ts.
export const INTEGRATION_NAMES = ['Mysterium', 'Presearch', 'Diiisco', 'SpaceAcres', 'Olostep'] as const

/**
 * Boot into the app-shell (skipping wizard). `useDevice` browser mock sets
 * registered:true, so App.tsx renders AppShell (Dashboard) immediately.
 */
export async function bootApp(page: Page) {
  await page.goto('/', { waitUntil: 'domcontentloaded' })
  await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('button', { name: 'Dashboard' })).toBeVisible()
}

/**
 * Boot into the wizard via the browser-only ?wizard=1 hint (see
 * src/hooks/useDevice.ts BROWSER_MOCK_UNREGISTERED path).
 */
export async function bootWizard(page: Page) {
  await page.goto('/?wizard=1', { waitUntil: 'domcontentloaded' })
  await expect(page.getByText('EDGE MINER', { exact: false }).first()).toBeVisible({ timeout: 15_000 })
}

/** Click a sidebar nav item by label. */
export async function nav(page: Page, label: NavLabel) {
  await page.getByRole('button', { name: label }).click()
}

/** Find an integration card by its display name. Uses the .ic class from IntCard. */
export function intCard(page: Page, name: (typeof INTEGRATION_NAMES)[number]) {
  return page.locator('.ic', { hasText: name }).first()
}

/** The toggle inside an integration card is the last <button> in the card. */
export function intToggle(page: Page, name: (typeof INTEGRATION_NAMES)[number]) {
  return intCard(page, name).getByRole('button').last()
}
