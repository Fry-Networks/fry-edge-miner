import { test as base, expect, chromium, type Page, type Browser } from '@playwright/test'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * FEM QA VM matrix — shared harness for specs under qa/vm/**.
 *
 * Fits ~/femqa/vm/HARNESS.md's existing tooling rather than inventing a
 * launch path: `vm-launch-fem <vm> '<exe path>'` must already have been run
 * (it launches the app with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=
 * --remote-debugging-port=9222 inside the guest and wires the guest-side
 * netsh portproxy so the HOST-published CDP port reaches it). This harness
 * only CONNECTS — it never launches anything and never touches the guest.
 *
 * CDP port mapping (HARNESS.md's port table, mirrored here since these
 * specs run as plain `npx playwright test`, outside the bash vm-* tooling
 * that would otherwise supply it via vmc_cdp_port):
 *   w11 -> 127.0.0.1:9223
 *   w10 -> 127.0.0.1:9224
 *
 * Select the target with FEMQA_VM=w11|w10 (default w11), or override the
 * whole URL with FEMQA_CDP_URL for a non-standard setup.
 */
const CDP_PORTS: Record<string, number> = { w11: 9223, w10: 9224 }

export function cdpUrl(): string {
  if (process.env.FEMQA_CDP_URL) return process.env.FEMQA_CDP_URL
  const vm = process.env.FEMQA_VM ?? 'w11'
  const port = CDP_PORTS[vm]
  if (!port) {
    throw new Error(`FEMQA_VM='${vm}' is not 'w11' or 'w10', and FEMQA_CDP_URL is not set`)
  }
  return `http://127.0.0.1:${port}`
}

export function vmName(): string {
  return process.env.FEMQA_VM ?? 'w11'
}

/**
 * Evidence directory for this run, matching vm-common.sh's
 * vmc_evidence_dir convention (~/femqa/evidence/vm/<matrix>/<vm>/<ts>/) so
 * matrix output lands next to every other VM QA artifact.
 */
export function evidenceDir(matrix: string): string {
  const ts = new Date().toISOString().replace(/[-:]/g, '').replace(/\.\d+Z$/, 'Z')
  const dir = join(process.env.HOME ?? '.', 'femqa', 'evidence', 'vm', matrix, vmName(), ts)
  mkdirSync(dir, { recursive: true })
  return dir
}

export function writeEvidence(matrix: string, filename: string, data: unknown): string {
  const dir = evidenceDir(matrix)
  const path = join(dir, filename)
  const text = typeof data === 'string' ? data : JSON.stringify(data, null, 2)
  writeFileSync(path, text, 'utf-8')
  return path
}

type Fixtures = { page: Page }
type WorkerFixtures = { vmBrowser: Browser }

export const test = base.extend<Fixtures, WorkerFixtures>({
  // Worker-scoped: one CDP connection per test file run, not per test —
  // connecting is the expensive/flaky step and every spec drives the same
  // already-running app instance in the guest.
  vmBrowser: [
    async ({}, use) => {
      const url = cdpUrl()
      let browser: Browser
      try {
        browser = await chromium.connectOverCDP(url)
      } catch (e) {
        throw new Error(
          `Could not connect to the FEM app over CDP at ${url}. ` +
            `Has vm-launch-fem been run against this VM (${vmName()})? ` +
            `Run \`vm-cdp ${vmName()}\` first to confirm the endpoint answers. ` +
            `Original error: ${e instanceof Error ? e.message : String(e)}`
        )
      }
      await use(browser)
      // browser.close() on a CDP-attached browser disconnects this client;
      // it does NOT terminate the remote app (Playwright docs) — the guest
      // keeps running exactly as vm-launch-fem left it.
      await browser.close()
    },
    { scope: 'worker' },
  ],

  page: async ({ vmBrowser }, use) => {
    const contexts = vmBrowser.contexts()
    if (contexts.length === 0) {
      throw new Error('CDP connected, but the app exposed no browser context — is the app actually showing a window?')
    }
    const context = contexts[0]
    const pages = context.pages()
    const page = pages.length > 0 ? pages[0] : await context.waitForEvent('page', { timeout: 10_000 })
    await page.bringToFront()
    await use(page)
    // Never close the app's own page — only disconnecting, per vmBrowser above.
  },
})

export { expect }

export const NAV_LABELS = ['Dashboard', 'Integrations', 'Rewards', 'Settings', 'Updates'] as const
export type NavLabel = (typeof NAV_LABELS)[number]

/** Click a sidebar nav item by label — same mechanism as tests/e2e/_shared.ts's nav(), reused
 *  here because this is the real app's real Sidebar, not a mock. */
export async function nav(page: Page, label: NavLabel) {
  await page.getByRole('button', { name: label }).click()
}

/**
 * Confirms the app is showing AppShell (Dashboard/Integrations/Rewards/
 * Settings/Updates), not the registration Wizard. Throws a clear,
 * actionable error if it is not — see qa/vm/README.md's "Known blocker"
 * section: AppShell is unreachable without a registered device, which is
 * out of policy for an unregistered QA VM unless the lead approves an
 * exception (flagged, not assumed).
 */
/**
 * Reads a StatCard's {value, sub, sub2} by label, without any test-only
 * hooks in StatCard.tsx (src/components/StatCard.tsx has none, and adding
 * one is out of scope for a VM-only harness). Relies on StatCard's actual,
 * stable JSX shape: an outer div whose direct children are [label-row-div,
 * value-div, sub-div?, sub2-div?] — see StatCard.tsx. If StatCard's
 * structure ever changes this helper's assumption should be revisited
 * alongside it.
 */
export async function readStatCard(
  page: Page,
  label: string
): Promise<{ value: string; sub: string | null; sub2: string | null }> {
  const labelEl = page.getByText(label, { exact: true }).first()
  const outer = labelEl.locator('xpath=ancestor::div[2]')
  const children = outer.locator('> div')
  const count = await children.count()
  const value = ((await children.nth(1).textContent()) ?? '').trim()
  const sub = count > 2 ? ((await children.nth(2).textContent()) ?? '').trim() : null
  const sub2 = count > 3 ? ((await children.nth(3).textContent()) ?? '').trim() : null
  return { value, sub, sub2 }
}

/**
 * Reads a `[label, value]` row from Dashboard's reward breakdown list (Base
 * reward / Staking mult / Required proportion / Second required boost /
 * Boost / BYOD factor — src/pages/Dashboard.tsx's inline .map(...)). Each
 * row is a flex div with exactly two spans: [label-span, value-span].
 */
export async function readBreakdownRow(page: Page, label: string): Promise<string> {
  const labelEl = page.getByText(label, { exact: true }).first()
  const row = labelEl.locator('xpath=..')
  const value = row.locator('span').nth(1)
  return ((await value.textContent()) ?? '').trim()
}

export async function assertInAppShell(page: Page) {
  const sidebarVisible = await page
    .getByRole('button', { name: 'Dashboard' })
    .isVisible({ timeout: 5_000 })
    .catch(() => false)
  if (!sidebarVisible) {
    throw new Error(
      'The app is not showing AppShell (no Dashboard nav button found) — it is almost certainly ' +
        'still on the registration Wizard, which is expected for an unregistered device per the ' +
        "run's hard rule (no miner_key, no device registration). M4/M5 cannot proceed past this " +
        'point without either registering a device (out of policy — flagged to the lead, not done ' +
        'here) or some other sanctioned way to reach AppShell. See qa/vm/README.md.'
    )
  }
}
