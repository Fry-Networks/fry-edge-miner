import { test, expect, bootApp, nav } from './_shared'

// Matrix F — Settings page.

test.describe('settings', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
    await nav(page, 'Settings')
    await expect(page.getByText('Device Identity', { exact: true })).toBeVisible({ timeout: 15_000 })
  })

  test('F1 all 4 section headers render', async ({ page }) => {
    for (const h of ['Device Identity', 'Reward Wallet', 'Preferences', 'About']) {
      await expect(page.getByText(h, { exact: true })).toBeVisible()
    }
  })

  test('F2 display name renders from device state', async ({ page }) => {
    // Default deviceName is 'nimble-swift-wolf' when phase is app (App.tsx)
    // Rendered in both sidebar footer AND Settings Device Identity — use .first()
    await expect(page.getByText('nimble-swift-wolf', { exact: true }).first()).toBeVisible()
  })

  test('F3 registration status shows Registered (browser mock has registered:true)', async ({ page }) => {
    await expect(page.getByText('Registered', { exact: true })).toBeVisible()
  })

  test('F4 four preference toggles present', async ({ page }) => {
    for (const l of ['Start on boot', 'Minimize to tray', 'Auto-update', 'Notifications']) {
      await expect(page.getByText(l, { exact: true })).toBeVisible()
    }
  })

  test('F5 About section shows Version + Platform + Tauri version', async ({ page }) => {
    await expect(page.getByText('Version', { exact: true })).toBeVisible()
    await expect(page.getByText('Platform', { exact: true })).toBeVisible()
    await expect(page.getByText('Tauri', { exact: true })).toBeVisible()
    // Actual version string from APP_VERSION (0.2.22 today) must appear.
    await expect(page.getByText(/^0\.2\.\d+$/).first()).toBeVisible()
    // "Windows x64" hardcoded — must render.
    await expect(page.getByText('Windows x64', { exact: true })).toBeVisible()
  })

  test('F6 Fry Networks / frynetworks.com / Discord footer renders', async ({ page }) => {
    await expect(page.getByText('Fry Networks', { exact: true })).toBeVisible()
    await expect(page.getByText('frynetworks.com', { exact: true })).toBeVisible()
    await expect(page.getByText('Discord', { exact: true })).toBeVisible()
  })

  test('F7 Deregister button renders when registered', async ({ page }) => {
    // Browser mock has registered:true → Deregister button visible.
    await expect(page.getByRole('button', { name: 'Deregister' })).toBeVisible()
  })
})
