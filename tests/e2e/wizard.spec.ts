import { test, expect, bootWizard } from './_shared'

// Matrix A — Wizard flow (4 steps, not 5 per plan reality check).
// browser-only ?wizard=1 override in useDevice.ts renders wizard.

test.describe('wizard', () => {
  test.beforeEach(async ({ page }) => {
    await bootWizard(page)
  })

  test('A1 Step0 Welcome renders + Get Started advances to Step1 Device & Wallet', async ({ page }) => {
    await expect(page.getByText('Welcome to Fry Edge Miner', { exact: true })).toBeVisible()
    await expect(page.getByText('System Checks', { exact: true })).toBeVisible()
    // What's-new box shows current APP_VERSION
    await expect(page.getByText(/What's new in v0\.2\.\d+/)).toBeVisible()
    const next = page.getByRole('button', { name: /Get Started/ })
    await expect(next).toBeVisible()
    await next.click()
    await expect(page.getByText('Device & Wallet', { exact: true })).toBeVisible({ timeout: 8_000 })
  })

  test('A2 Step1 — Continue is disabled until miner key + wallet are BOTH valid', async ({ page }) => {
    await page.getByRole('button', { name: /Get Started/ }).click()
    await expect(page.getByText('Device & Wallet', { exact: true })).toBeVisible()

    const cont = page.getByRole('button', { name: /Continue/ })
    await expect(cont).toBeDisabled()

    // Invalid miner key
    const keyInput = page.locator('input').nth(0)
    await keyInput.fill('not-a-fem-key')
    await expect(page.getByText('Must be FEM- followed by 32 alphanumeric characters')).toBeVisible()
    await expect(cont).toBeDisabled()

    // Valid miner key
    await keyInput.fill('FEM-00000000000000000000000000000000')
    await expect(page.getByText('Valid miner key', { exact: true })).toBeVisible()
    await expect(cont).toBeDisabled() // still disabled — wallet not entered

    // Wallet too short → invalid state message + still disabled
    const addrInput = page.locator('input').nth(1)
    await addrInput.fill('SHORT')
    await expect(page.getByText(/Not valid — \d+\/58 chars/)).toBeVisible()
    await expect(cont).toBeDisabled()

    // Wallet valid (58 chars, base32 [A-Z2-7])
    const goodWallet = 'A'.repeat(58)
    await addrInput.fill(goodWallet)
    await expect(page.getByText('Valid Algorand address', { exact: true })).toBeVisible()
    await expect(cont).toBeEnabled()
  })

  test('A3 Step1 → Step2 Review shows the 5 partner integrations', async ({ page }) => {
    await page.getByRole('button', { name: /Get Started/ }).click()
    await page.locator('input').nth(0).fill('FEM-00000000000000000000000000000000')
    await page.locator('input').nth(1).fill('A'.repeat(58))
    await page.getByRole('button', { name: /Continue/ }).click()
    // Step2Review renders — must list all 5 integrations by name.
    for (const n of ['Mysterium', 'Presearch', 'Diiisco', 'SpaceAcres', 'Olostep']) {
      await expect(page.getByText(n, { exact: true }).first()).toBeVisible({ timeout: 8_000 })
    }
  })

  test('A4 Back button from Step1 returns to Welcome', async ({ page }) => {
    await page.getByRole('button', { name: /Get Started/ }).click()
    await expect(page.getByText('Device & Wallet', { exact: true })).toBeVisible()
    await page.getByRole('button', { name: /Back/ }).click()
    await expect(page.getByText('Welcome to Fry Edge Miner', { exact: true })).toBeVisible()
  })

  test('A5 rapid Get-Started double-click does not crash the wizard', async ({ page, errors }) => {
    // Regression cover for a hypothesized step-skip race (initially thought a bug,
    // proven benign under React 18 batching: the second click either targets the
    // freshly-mounted Step1 button or lands on empty area, never fast-forwarding
    // through Step1). Assert the app still renders SOME wizard content + no crash.
    const btn = page.getByRole('button', { name: /Get Started/ })
    await btn.dblclick()
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()
    expect(errors.filter((e) => /Uncaught|crash|is not a function/i.test(e)), errors.join('\n')).toEqual([])
  })
})
