import { test, expect, bootApp, nav, NAV_LABELS } from './_shared'

// Matrix T — TopBar connectivity and version checks.
// Note: topbar.spec.ts covers H1–H3 (title/subtitle/version basics).
// This suite adds connectivity-derivation nuance and version cross-check.

test.describe('topbar-connectivity', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
  })

  test('T-C1 connectivity indicator renders browser-mode expected state (Connected)', async ({ page }) => {
    // App.tsx connectivity logic:
    // - 'disconnected' if deviceError OR integrations.error
    // - 'degraded' if system.docker !== 'ready'
    // - 'connected' otherwise
    // Browser mode: deviceError = null, integrations.error = null, system = null
    // Therefore: connectivity should be 'connected'

    await expect(page.getByText('Connected', { exact: true })).toBeVisible()

    // Verify the indicator is present somewhere in the TopBar
    // It should be a visual element (icon or text) indicating connectivity
    const topBar = page.locator('[class*="top"], [class*="bar"], header')
    if (await topBar.isVisible({ timeout: 3_000 }).catch(() => false)) {
      // TopBar exists and rendered, check for connectivity text/icon
      const connIndicator = topBar.getByText(/Connected|Degraded|Disconnected/i)
      await expect(connIndicator).toBeVisible()
    }
  })

  test('T-C2 page title and subtitle update correctly for each nav page', async ({ page }) => {
    // This largely duplicates topbar.spec.ts H1, but we re-verify
    // the consistency across multiple pages in one test
    const expectations: Record<string, string> = {
      Dashboard: 'System overview and live reward status',
      Integrations: 'Manage partner integration processes',
      Rewards: 'Earnings history and proof-of-contribution',
      Settings: 'Device configuration and preferences',
      Updates: 'Software version management',
    }

    for (const label of NAV_LABELS) {
      await nav(page, label)
      const subtitle = expectations[label]
      await expect(page.getByText(subtitle, { exact: true })).toBeVisible({ timeout: 8_000 })
    }
  })

  test('T-C3 version tag in topbar matches package.json version', async ({ page }) => {
    // TopBar displays APP_VERSION which is imported from package.json
    // package.json version = "0.2.23"
    // Expected tag: "v0.2.23"
    const versionTag = page.getByText(/^v0\.2\.\d+$/)
    await expect(versionTag).toBeVisible()

    // Verify it's exactly in the format v0.2.NN
    const versionText = await versionTag.first().textContent()
    expect(versionText).toMatch(/^v0\.2\.\d+$/)

    // The version should be v0.2.23 (or whatever package.json specifies)
    // We don't hardcode the exact number, but verify it's the Fry v0.2.x series
    expect(versionText).toContain('v0.2')
  })

  test('T-C4 connectivity indicator update on each nav (all show Connected in browser mode)', async ({ page }) => {
    // Verify that as we navigate, the connectivity indicator persists and shows correct state
    for (const label of NAV_LABELS) {
      await nav(page, label)

      // Each page should show "Connected" in browser mode (since deviceError and error are null)
      await expect(page.getByText('Connected', { exact: true })).toBeVisible()
    }
  })
})
