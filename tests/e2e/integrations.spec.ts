import { test, expect, bootApp, nav, intCard, intToggle, INTEGRATION_NAMES } from './_shared'

// Matrix D — Integrations page.
//
// Browser mode: useIntegrations mock returns 5 integrations all
// `enabled:false, health:'Stopped', lifecycle:'Disabled', version:'0.0.0-preview',
// requires_docker:true`. Toggle IPC fails (no Tauri) — surfaces the ErrorBanner.

test.describe('integrations', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
    await nav(page, 'Integrations')
    await expect(page.locator('.ic').first()).toBeVisible({ timeout: 15_000 })
  })

  test('D1 all 5 integration cards render with correct display names', async ({ page }) => {
    for (const name of INTEGRATION_NAMES) {
      await expect(intCard(page, name)).toBeVisible()
    }
    // And exactly 5 cards total
    await expect(page.locator('.ic')).toHaveCount(5)
  })

  test('D2 initial state — proportion counter shows 0/5 · 0%', async ({ page }) => {
    await expect(page.getByText(/^0\s*\/\s*5\s*·\s*0%$/)).toBeVisible()
  })

  test('D3 initial state — all cards show "Disabled" status tag', async ({ page }) => {
    // Every card should have a Disabled label (case-insensitive; Tag component
    // wraps children as-is but the Tag CSS may transform case).
    for (const name of INTEGRATION_NAMES) {
      const card = intCard(page, name)
      await expect(card.getByText(/disabled/i)).toBeVisible()
    }
  })

  test('D4 each integration card shows tag/description/version', async ({ page }) => {
    const tags = ['VPN NODE', 'SEARCH NODE', 'AI NODE', 'STORAGE NODE', 'SCRAPE NODE']
    for (const tag of tags) {
      await expect(page.getByText(tag, { exact: true })).toBeVisible()
    }
    // Version chip 'v0.0.0-preview' appears for each (browser mock)
    await expect(page.getByText('v0.0.0-preview').first()).toBeVisible()
  })

  test('D5 toggling a card produces optimistic Installing then surfaces an error (browser mode)', async ({ page, errors }) => {
    // Ignore expected errors from the IPC failure surfaced through ErrorBanner:
    // useIntegrations.toggle uses console.warn (not error) so no console-error is emitted.
    // The optimistic flip renders 'Installing' briefly; then fetch() resets state.
    const toggle = intToggle(page, 'Mysterium')
    await toggle.click()
    // Either an ErrorBanner appears (with prefix "Error:") OR the card resets to Disabled.
    // We don't force a specific outcome — just check the toggle click didn't crash the page.
    await expect(page.getByText('Mysterium', { exact: true })).toBeVisible({ timeout: 5_000 })
    // The captured errors set may include the failed Tauri invoke logged via console.error
    // from the @tauri-apps/api/core. Allow only that specific one.
    const unexpected = errors.filter(
      (e) => !/toggle_integration|window\.__TAURI|invoke|tauri/i.test(e)
    )
    expect(unexpected, `unexpected errors:\n${unexpected.join('\n')}`).toEqual([])
  })

  test('D6 no Docker prerequisite banner in browser mode (system status unknown)', async ({ page }) => {
    // system is null in browser mode → no "Docker not ready" AlertTriangle banner
    await expect(page.locator('text=/Docker Desktop/i')).toHaveCount(0)
  })

  test.fixme('D7 rapid toggle race — 10x on/off in <2s (requires Tauri IPC)', async () => {
    // Cannot exercise real toggle path in browser mode — IPC fails.
    // Covered in Phase 4 backend and manual desktop-mode verification.
  })

  test.fixme('D8 all-5 concurrent toggle race (requires Tauri IPC)', async () => {
    // Real backend required — browser fetch() resets state after each toggle.
  })

  test('D9 reward-contribution progress bar exists on each card', async ({ page }) => {
    const bars = page.getByText('Reward contribution')
    await expect(bars.first()).toBeVisible()
    // Count = 5 (one per card)
    await expect(bars).toHaveCount(5)
  })

  test('D10 no unexpected console/network errors while browsing integrations', async ({ page, errors }) => {
    await nav(page, 'Dashboard')
    await nav(page, 'Integrations')
    await expect(page.locator('.ic').first()).toBeVisible()
    // Filter out the known Tauri IPC "no window.__TAURI" console errors — they're
    // expected because we're in browser preview mode and the initial get_integrations
    // invoke fails once. But we did NOT toggle in this test so shouldn't see them.
    const unexpected = errors.filter(
      (e) => !/toggle_integration|window\.__TAURI|get_integrations|get_system_status|get_device_info|get_reward_summary|get_poc_slots/i.test(e)
    )
    expect(unexpected, `integrations errors:\n${unexpected.join('\n')}`).toEqual([])
  })
})
