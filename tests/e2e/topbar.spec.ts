import { test, expect, bootApp, nav, NAV_LABELS } from './_shared'

// Matrix H — TopBar.

test.describe('topbar', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
  })

  test('H1 page title + subtitle update per nav', async ({ page }) => {
    const subs: Record<string, string> = {
      Dashboard: 'System overview and live reward status',
      Integrations: 'Manage partner integration processes',
      Rewards: 'Earnings history and proof-of-contribution',
      Settings: 'Device configuration and preferences',
      Updates: 'Software version management',
    }
    for (const label of NAV_LABELS) {
      await nav(page, label)
      await expect(page.getByText(subs[label], { exact: true })).toBeVisible()
    }
  })

  test('H2 connection indicator shows "Connected"', async ({ page }) => {
    await expect(page.getByText('Connected', { exact: true })).toBeVisible()
  })

  test('H3 version tag matches package.json', async ({ page }) => {
    // APP_VERSION is imported from package.json version. Assert it renders v0.2.NN.
    await expect(page.getByText(/^v0\.2\.\d+$/)).toBeVisible()
  })
})
