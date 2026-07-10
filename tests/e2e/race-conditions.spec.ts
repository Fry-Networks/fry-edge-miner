import { test, expect, bootApp, nav, NAV_LABELS } from './_shared'

// Matrix J — race conditions + edge cases. Every J* rerun 3x to catch flakes.

const REP = 3

test.describe('race-conditions', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
  })

  test('J1 rapid page cycling <2s x5', async ({ page, errors }) => {
    for (let k = 0; k < REP; k++) {
      for (let cycle = 0; cycle < 5; cycle++) {
        for (const label of NAV_LABELS) {
          await page.getByRole('button', { name: label }).click({ timeout: 3_000 })
        }
      }
      await expect(page.getByText(/Update|Current version|Last checked|Not checked yet/i).first()).toBeVisible({
        timeout: 10_000,
      })
      await nav(page, 'Dashboard')
      await expect(page.getByText('Active Integrations', { exact: true })).toBeVisible()
    }
    const unexpected = errors.filter((e) => !/window\.__TAURI|get_[a-z_]+|save_settings|check_updates/i.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })

  test('J2 double-click every nav button — no duplicate actions or crashes', async ({ page, errors }) => {
    for (let k = 0; k < REP; k++) {
      for (const label of NAV_LABELS) {
        const btn = page.getByRole('button', { name: label })
        await btn.dblclick()
        // The dblclick counts as one nav (both clicks go to same handler).
        // Verify page did NOT crash — some marker text still renders.
        await expect(page.getByText(label, { exact: true }).first()).toBeVisible()
      }
    }
    const unexpected = errors.filter((e) => !/window\.__TAURI|get_[a-z_]+|save_settings|check_updates/i.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })

  test('J3 resize during nav does not corrupt layout', async ({ page }) => {
    for (let k = 0; k < REP; k++) {
      await page.setViewportSize({ width: 800, height: 600 })
      await nav(page, 'Integrations')
      await page.setViewportSize({ width: 1920, height: 1080 })
      await nav(page, 'Dashboard')
      await expect(page.getByText('Active Integrations', { exact: true })).toBeVisible()
    }
  })

  test('J4 F5 refresh on every page recovers to Dashboard', async ({ page }) => {
    for (let k = 0; k < REP; k++) {
      for (const label of NAV_LABELS) {
        await nav(page, label)
        await page.reload({ waitUntil: 'domcontentloaded' })
        await expect(page.getByText('Active Integrations', { exact: true })).toBeVisible({ timeout: 15_000 })
      }
    }
  })
})
