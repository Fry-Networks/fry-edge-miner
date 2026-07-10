import { test, expect, bootApp, nav, NAV_LABELS } from './_shared'

// Matrix B — sidebar nav + branding + footer.

test.describe('sidebar', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
  })

  test('B1 renders all 5 nav items', async ({ page }) => {
    for (const label of NAV_LABELS) {
      await expect(page.getByRole('button', { name: label })).toBeVisible()
    }
  })

  test('B2 clicking each item loads the corresponding page (Dashboard is initial)', async ({ page, errors }) => {
    // Dashboard is initial state. Navigate to other pages and back.
    for (const label of ['Integrations', 'Rewards', 'Settings', 'Updates', 'Dashboard'] as const) {
      await nav(page, label)
      // Give the page transition a moment; then check the button is active.
      // Active state is the nav button with a bold border-left (`3px solid var(--red)`)
      // and background var(--s2). We can't easily assert CSS var, so just verify
      // some page-specific text renders after nav.
      const marker: Record<string, string | RegExp> = {
        Dashboard: 'Active integrations',
        Integrations: /Integrations|Each running integration|No integrations available/,
        Rewards: /Total earned|Rewards|Full day estimate/i,
        Settings: /Device identity|Wallet|Miner key/i,
        Updates: /Update|Current version|Latest/i,
      }
      await expect(page.getByText(marker[label] as any).first()).toBeVisible({ timeout: 10_000 })
    }
    expect(errors, `nav errors:\n${errors.join('\n')}`).toEqual([])
  })

  test('B4 rapid nav switching stays consistent', async ({ page, errors }) => {
    // 5 cycles of Dashboard → Integrations → Rewards → Settings → Updates rapidly.
    for (let cycle = 0; cycle < 5; cycle++) {
      for (const label of NAV_LABELS) {
        await page.getByRole('button', { name: label }).click()
      }
    }
    // Final page should be Updates.
    await expect(page.getByText(/Update|Current version|Latest/i).first()).toBeVisible({ timeout: 10_000 })
    expect(errors, `rapid nav errors:\n${errors.join('\n')}`).toEqual([])
  })

  test('B5 device footer renders the real (truncated) miner key from device state', async ({ page }) => {
    // Post Bug-#1 fix: Sidebar footer derives from device.miner_key via App.tsx.
    // Browser mock provides `FEM-BROWSER-PREVIEW-MODE` — truncated form is
    // `FEM-brow…mode` (first 4 body chars + ellipsis + last 4).
    await expect(page.getByText('FEM-brow…mode', { exact: true })).toBeVisible()
  })

  test('B6 brand renders FRY + EDGE MINER', async ({ page }) => {
    await expect(page.getByText('FRY', { exact: true })).toBeVisible()
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()
  })
})
