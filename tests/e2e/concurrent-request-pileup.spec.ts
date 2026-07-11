import { test, expect, bootApp, nav } from './_shared'

// Matrix C — Concurrent request pileup and race conditions.
const KNOWN_TAURI_NOISE = /window\.__TAURI|invoke|register_device|toggle_integration|get_updates|fonts\.gstatic|REQFAIL|ERR_ABORTED/i

test.describe('concurrent-request-pileup', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
  })

  test('C-R1 delayed network responses + rapid nav 10× → correct final page, no stale content', async ({ page, errors }) => {
    // Set up request interception with delayed fulfillment
    // This simulates slow network conditions where nav happens before response arrives
    await page.route('**/*', async (route, request) => {
      // Delay all requests by 300ms
      await new Promise((resolve) => setTimeout(resolve, 300))
      try {
        await route.continue()
      } catch {
        // If continuation fails (e.g., navigation), catch and allow
      }
    })

    // Rapid nav cycling (10 cycles through all 5 pages = 50 clicks)
    const pages = ['Dashboard', 'Integrations', 'Rewards', 'Settings', 'Updates']
    for (let cycle = 0; cycle < 10; cycle++) {
      for (const pageName of pages) {
        await nav(page, pageName)
        // Don't wait for full load — immediately proceed to next nav
      }
    }

    // Finally, nav to Dashboard and wait for it to fully settle
    await nav(page, 'Dashboard')
    await expect(page.getByText('Active Integrations', { exact: true })).toBeVisible({ timeout: 15_000 })

    // Verify no stale content from other pages
    // (e.g., if we landed on Dashboard, shouldn't see "Manage partner integrations")
    const integrationsSubtitle = page.getByText('Manage partner integration processes', { exact: true })
    await expect(integrationsSubtitle).not.toBeVisible()

    // No unhandled rejections
    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })

  test('C-R2 toggle integration then immediately nav away mid-flight → no unhandled rejection', async ({ page, errors }) => {
    // Navigate to Integrations
    await nav(page, 'Integrations')
    await expect(page.locator('.ic').first()).toBeVisible()

    // Set up request interception to delay toggle response
    await page.route('**/*', async (route, request) => {
      // For toggle requests, delay significantly
      await new Promise((resolve) => setTimeout(resolve, 1000))
      try {
        await route.continue()
      } catch {
        // Navigation away — ignore continuation error
      }
    })

    // Get a toggle button and click it (but don't await fully)
    const mysteriumToggle = page.locator('.ic').filter({ hasText: 'Mysterium' }).getByRole('button').last()
    const clickPromise = mysteriumToggle.click().catch(() => {})

    // Immediately nav away to Dashboard (mid-flight of toggle)
    await page.waitForTimeout(100) // Brief pause to ensure toggle request sent
    await nav(page, 'Dashboard')

    // Wait for Dashboard to settle
    await expect(page.getByText('Active Integrations', { exact: true })).toBeVisible({ timeout: 15_000 })

    // Toggle click should eventually complete or fail gracefully
    await clickPromise

    // No unhandled rejections should be captured
    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })

  test('C-R3 on Step3 error, click Retry then immediately F5 → clean reload without hanging', async ({ page, errors }) => {
    // Navigate to wizard
    await page.goto('/?wizard=1', { waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()

    // Get to Step3 (Install)
    await page.getByRole('button', { name: /Get Started/ }).click()
    await page.locator('input').nth(0).fill('FEM-55555555555555555555555555555555')
    await page.locator('input').nth(1).fill('F'.repeat(58))
    await page.getByRole('button', { name: /Continue/ }).click()
    await expect(page.getByText('Mysterium', { exact: true }).first()).toBeVisible()

    // Click Install to trigger Step3 (which will fail in browser mode)
    await page.getByRole('button', { name: /Install/ }).click()

    // Set up slow response
    await page.route('**/*', async (route, request) => {
      await new Promise((resolve) => setTimeout(resolve, 500))
      try {
        await route.continue()
      } catch {
        // Ignore
      }
    })

    // Verify Retry button is visible (mandatory)
    const retryBtn = page.getByRole('button', { name: /Retry/i })
    await expect(retryBtn).toBeVisible({ timeout: 3_000 })

    // Click Retry (but don't fully await response)
    const retryPromise = retryBtn.click().catch(() => {})

    // Immediately F5 (mid-flight of Retry)
    await page.waitForTimeout(100)
    await page.reload({ waitUntil: 'domcontentloaded' })

    // Page should reload cleanly
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()

    // Retry promise should eventually settle
    await retryPromise

    // No hanging or unhandled state
    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })
})
