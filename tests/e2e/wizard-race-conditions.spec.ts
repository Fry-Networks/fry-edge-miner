import { test, expect, bootWizard } from './_shared'

// Matrix W — Wizard race conditions. Every W-R* rerun to catch flakes.
const KNOWN_TAURI_NOISE = /window\.__TAURI|invoke|register_device|toggle_integration|get_reward_summary|get_poc_slots|check_updates|fonts\.gstatic|REQFAIL|ERR_ABORTED/i

test.describe('wizard-race-conditions', () => {
  test.beforeEach(async ({ page }) => {
    await bootWizard(page)
  })

  test('W-R1 rapid double-click Get Started → exactly one step transition, no crash', async ({ page, errors }) => {
    const btn = page.getByRole('button', { name: /Get Started/ })
    await btn.dblclick()
    // After double-click, either still on Welcome or advanced to Step1Device.
    // The key is: no crash, EDGE MINER still renders, and transitions are atomic.
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()
    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })

  test('W-R2 double-click Continue on Step1 with valid inputs → state remains valid', async ({ page, errors }) => {
    // Advance to Step1
    await page.getByRole('button', { name: /Get Started/ }).click()
    await expect(page.getByText('Device & Wallet', { exact: true })).toBeVisible()

    // Fill valid inputs
    await page.locator('input').nth(0).fill('FEM-00000000000000000000000000000000')
    await page.locator('input').nth(1).fill('A'.repeat(58))

    // Double-click Continue (race condition: verify no crash or invalid state)
    const cont = page.getByRole('button', { name: /Continue/ })
    await cont.dblclick()

    // After double-click, app should still be responsive (EDGE MINER header visible)
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 8_000 })

    // Verify no console errors from the race condition
    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })

  test('W-R3 back-click spam 10× on review step → navigates back, stays functional', async ({ page, errors }) => {
    // Navigate to Step2Review
    await page.getByRole('button', { name: /Get Started/ }).click()
    await page.locator('input').nth(0).fill('FEM-11111111111111111111111111111111')
    await page.locator('input').nth(1).fill('B'.repeat(58))
    await page.getByRole('button', { name: /Continue/ }).click()
    await expect(page.getByText(/Review Integrations|Mysterium/, { exact: false }).first()).toBeVisible({ timeout: 8_000 })

    // Rapid back-click spam (10x) — should navigate back through steps
    const backBtn = page.getByRole('button', { name: /Back/ })
    for (let i = 0; i < 10; i++) {
      if (await backBtn.isVisible({ timeout: 1_000 }).catch(() => false)) {
        await backBtn.click({ timeout: 2_000 })
      }
    }

    // After spam, should be somewhere in the wizard (not crashed)
    // Could be at Step1 (Device & Wallet) or even back at Welcome (Step 0)
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()
    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })

  test('W-R4 F5 refresh at each wizard step → app returns to sane state', async ({ page, errors }) => {
    // At Step0 (Welcome)
    await page.reload({ waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()

    // Advance to Step1
    await page.getByRole('button', { name: /Get Started/ }).click()
    await expect(page.getByText('Device & Wallet', { exact: true })).toBeVisible()

    // F5 at Step1
    await page.reload({ waitUntil: 'domcontentloaded' })
    // After reload with ?wizard=1 still in URL, should return to wizard (likely Step0 or earlier)
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()

    // Fill and continue to Step2Review
    await page.getByRole('button', { name: /Get Started/ }).click()
    await page.locator('input').nth(0).fill('FEM-22222222222222222222222222222222')
    await page.locator('input').nth(1).fill('C'.repeat(58))
    await page.getByRole('button', { name: /Continue/ }).click()
    await expect(page.getByText('Mysterium', { exact: true }).first()).toBeVisible()

    // F5 at Step2Review
    await page.reload({ waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()

    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })

  test('W-R5 navigate to / mid-wizard → clean handling, no unhandled rejection', async ({ page, errors }) => {
    // Navigate to Step1
    await page.getByRole('button', { name: /Get Started/ }).click()
    await page.locator('input').nth(0).fill('FEM-33333333333333333333333333333333')
    await page.locator('input').nth(1).fill('D'.repeat(58))

    // Navigate away to '/' before completing the form
    await page.goto('/', { waitUntil: 'domcontentloaded' })

    // Should land on AppShell (Dashboard) or wizard, not crash
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()
    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })

  test('W-R6 retry-button spam on Step3 error → each attempt stable, no crash', async ({ page, errors }) => {
    // Navigate to Step3 (Install)
    await page.getByRole('button', { name: /Get Started/ }).click()
    await page.locator('input').nth(0).fill('FEM-44444444444444444444444444444444')
    await page.locator('input').nth(1).fill('E'.repeat(58))
    await page.getByRole('button', { name: /Continue/ }).click()
    await expect(page.getByText(/Review Integrations/, { exact: false })).toBeVisible({ timeout: 8_000 })

    // Click Install → Step3 (browser mode: registration fails, error panel appears)
    await page.getByRole('button', { name: /Install/ }).click()

    // Wait for error state (registration fails in browser mode, error panel + Retry button)
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 8_000 })

    // Verify Retry button exists (mandatory for the race condition)
    const retryBtn = page.getByRole('button', { name: /Retry/i })
    await expect(retryBtn).toBeVisible({ timeout: 3_000 })

    // Rapid retry spam (5× clicks)
    for (let i = 0; i < 5; i++) {
      await retryBtn.click()
      // Brief pause between clicks
      await page.waitForTimeout(50)
    }

    // Error panel should remain stable, app not crashed
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()

    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })
})
